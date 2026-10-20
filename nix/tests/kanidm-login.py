"""Log in to Lodestar's router entrance through Kanidm, without a browser.

kanidm-login --server http://<manager>:8081 --kanidm https://<kanidm>
             --user <name> --password-file <file>

prints the `__Host-lodestar_session` value. With --id-token-claims (and
--client-secret-file, --redirect-uri) it runs its own authorization-code
flow as the OAuth2 client instead and prints the ID token's claims as JSON
(the test reads the `groups` claim Kanidm sends from it).

Cookies are sent by hand: `__Host-`/`Secure` cookies are never stored for
a plain-http server. On a failure it exits 1 and names the stage.
"""

import argparse
import base64
import hashlib
import json
import secrets
import sys
import urllib.error
import urllib.parse
import urllib.request

LOGIN_COOKIE = "__Host-lodestar_login"
SESSION_COOKIE = "__Host-lodestar_session"
KANIDM_SESSION_HEADER = "X-KANIDM-AUTH-SESSION-ID"


class StageFailed(Exception):
    def __init__(self, stage, detail):
        super().__init__(f"{stage}: {detail}")
        self.stage = stage


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


OPENER = urllib.request.build_opener(NoRedirect())


def request(stage, url, method="GET", body=None, headers=None):
    """Sends one request; returns (status, headers, body). 4xx/5xx fail."""
    headers = dict(headers or {})
    data = None
    if body is not None:
        if isinstance(body, (bytes, str)):
            data = body.encode() if isinstance(body, str) else body
        else:
            data = json.dumps(body).encode()
            headers.setdefault("Content-Type", "application/json")
    req = urllib.request.Request(url, data=data, method=method,
                                 headers=headers)
    try:
        with OPENER.open(req, timeout=30) as response:
            return response.status, response.headers, response.read()
    except urllib.error.HTTPError as error:
        if 300 <= error.code < 400:
            # A redirect's body is not needed (and reading it can block).
            error.close()
            return error.code, error.headers, b""
        text = error.read().decode(errors="replace")[:500]
        raise StageFailed(stage, f"HTTP {error.code}: {text}")
    except OSError as error:
        raise StageFailed(stage, str(error))


def set_cookie(headers, name):
    """The value of the Set-Cookie for `name`, or None."""
    for line in headers.get_all("Set-Cookie") or []:
        pair = line.split(";", 1)[0]
        key, _, value = pair.partition("=")
        if key.strip() == name:
            return value.strip()
    return None


def query_of(url):
    return dict(urllib.parse.parse_qsl(urllib.parse.urlsplit(url).query))


def kanidm_token(kanidm, user, password):
    """Password login against Kanidm's API; returns a bearer token."""
    url = f"{kanidm}/v1/auth"
    init = {"init2": {"username": user, "issue": "token"}}
    _, headers, body = request("kanidm init", url, "POST", {"step": init})
    session = headers.get(KANIDM_SESSION_HEADER)
    if not session:
        raise StageFailed("kanidm init", f"no session id: {body!r}")
    auth = {KANIDM_SESSION_HEADER: session}
    steps = [
        ("kanidm begin", {"begin": "password"}),
        ("kanidm password", {"cred": {"password": password}}),
    ]
    state = {}
    for stage, step in steps:
        _, _, body = request(stage, url, "POST", {"step": step}, auth)
        state = json.loads(body)["state"]
        if "denied" in state:
            raise StageFailed(stage, f"denied: {state['denied']}")
    if "success" not in state:
        raise StageFailed("kanidm password", f"unexpected state {state}")
    return state["success"]


def authorise(kanidm, token, authorize_query):
    """Kanidm's authorise (and consent) step; returns the redirect URL."""
    bearer = {"Authorization": f"Bearer {token}"}
    _, headers, body = request("authorise", f"{kanidm}/oauth2/authorise",
                               "POST", authorize_query, bearer)
    location = headers.get("Location")
    if location:
        return location
    answer = json.loads(body)
    if "ConsentRequested" not in answer:
        raise StageFailed("authorise", f"unexpected answer {answer}")
    consent = answer["ConsentRequested"]["consent_token"]
    # The body is the consent token as a JSON string.
    _, headers, _ = request("consent", f"{kanidm}/oauth2/authorise/permit",
                            "POST", json.dumps(consent),
                            {**bearer, "Content-Type": "application/json"})
    location = headers.get("Location")
    if not location:
        raise StageFailed("consent", "no redirect")
    return location


def code_and_state(location):
    redirect = query_of(location)
    if "code" not in redirect:
        raise StageFailed("redirect", f"no code in {location}")
    return redirect["code"], redirect.get("state", "")


def lodestar_login(args, password):
    status, headers, _ = request("lodestar login", f"{args.server}/auth/login")
    location = headers.get("Location")
    login_state = set_cookie(headers, LOGIN_COOKIE)
    if status != 303 or not location or not login_state:
        raise StageFailed("lodestar login", f"HTTP {status}, no redirect")
    token = kanidm_token(args.kanidm, args.user, password)
    code, state = code_and_state(authorise(args.kanidm, token,
                                           query_of(location)))
    callback = urllib.parse.urlencode({"code": code, "state": state})
    status, headers, body = request(
        "lodestar callback", f"{args.server}/auth/callback?{callback}",
        headers={"Cookie": f"{LOGIN_COOKIE}={login_state}"})
    session = set_cookie(headers, SESSION_COOKIE)
    if status != 303 or not session:
        raise StageFailed("lodestar callback",
                          f"HTTP {status}, no session cookie")
    return session


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def id_token_claims(args, password):
    """Runs the flow as the OAuth2 client; returns the ID token's claims."""
    with open(args.client_secret_file) as file:
        client_secret = file.read().strip()
    verifier = b64url(secrets.token_bytes(32))
    query = {
        "response_type": "code",
        "client_id": args.client_id,
        "redirect_uri": args.redirect_uri,
        "scope": "openid profile groups",
        "state": b64url(secrets.token_bytes(16)),
        "nonce": b64url(secrets.token_bytes(16)),
        "code_challenge": b64url(hashlib.sha256(verifier.encode()).digest()),
        "code_challenge_method": "S256",
    }
    token = kanidm_token(args.kanidm, args.user, password)
    code, _ = code_and_state(authorise(args.kanidm, token, query))
    basic = base64.b64encode(f"{args.client_id}:{client_secret}".encode())
    form = urllib.parse.urlencode({
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": args.redirect_uri,
        "code_verifier": verifier,
    })
    _, _, body = request(
        "token", f"{args.kanidm}/oauth2/token", "POST", form,
        {"Content-Type": "application/x-www-form-urlencoded",
         "Authorization": "Basic " + basic.decode()})
    payload = json.loads(body)["id_token"].split(".")[1]
    return json.loads(base64.urlsafe_b64decode(payload + "=" * 3))


def main():
    parser = argparse.ArgumentParser(prog="kanidm-login")
    parser.add_argument("--server", help="Lodestar's router entrance URL")
    parser.add_argument("--kanidm", required=True)
    parser.add_argument("--user", required=True)
    parser.add_argument("--password-file", required=True)
    parser.add_argument("--id-token-claims", action="store_true")
    parser.add_argument("--client-id", default="lodestar")
    parser.add_argument("--client-secret-file")
    parser.add_argument("--redirect-uri")
    args = parser.parse_args()
    if args.id_token_claims:
        if not (args.client_secret_file and args.redirect_uri):
            parser.error("--id-token-claims needs --client-secret-file "
                         "and --redirect-uri")
    elif not args.server:
        parser.error("--server is required")
    with open(args.password_file) as file:
        password = file.read().strip()
    try:
        if args.id_token_claims:
            print(json.dumps(id_token_claims(args, password)))
        else:
            print(lodestar_login(args, password))
    except StageFailed as error:
        print(f"kanidm-login: failed at {error}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
