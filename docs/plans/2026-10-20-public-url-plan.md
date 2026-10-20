# Lodestar on lodestar.vanwaaij.com: plan

What this delivers: Lodestar at **`https://lodestar.vanwaaij.com`**, from anywhere.

- **Anyone** sees what the homelab hosts and whether it's up: name, group, description,
  icon, up/down. No links, no machines, no error messages.
- **Logged in** (with your vanwaaij.com account, group `lodestar-users`): the whole
  dashboard, read-only.
- **Admins** (group `admins`) can also apply, rebuild and promote, but **only when they come
  in over Tailscale**. Over the internet the buttons are greyed out with "only over
  Tailscale".
- The current tailnet address and the `lodestar` command keep working exactly as now.

Design: `2026-10-20-public-url-design.md`. Technical version for the implementing agents:
`2026-10-20-public-url-plan-technical.md`.

## Who does what

- **Claude** writes the Lodestar changes and the tests, the settings in `~/.flake`, the
  Tailscale policy, and checks everything.
- **You:**
  1. Create one Tailscale Service, `lodestar`, in the admin console (step 6).
  2. Approve the 1Password prompt once, for Lodestar's Kanidm secret (step 5).
  3. Try it: logged out, logged in from outside, logged in over Tailscale (step 7).
- Nothing gets pushed to GitHub without your OK.

## Step 1: The public Services list

Lodestar gets a separate, stripped-down list for visitors who aren't logged in: only
name, group, description, icon and up/down with since when. It's its own list on purpose,
so a later change to the full Services tab can't leak anything to the public one.

## Step 2: Logging in with Kanidm

A "Log in" button sends you to `id.vanwaaij.com` and back. Lodestar checks which groups
you're in (`lodestar-users` to read, `admins` to act) and keeps you logged in for 12 hours.
Someone in neither group gets "no access". "Log out" ends it.

## Step 3: The entrance for the router

Lodestar gets a second entrance that only the router may use (it refuses everyone else,
and the firewall blocks them too). Through it, the rules from the top of this plan apply.
The router tells Lodestar whether you came in over Tailscale; Lodestar believes that only
from the router.

## Step 4: The website

The page knows three states: logged out (only the Services list and "Log in"), logged in
(everything, without action buttons for readers), and admin (action buttons, greyed out
with "only over Tailscale" when you're not on Tailscale).

**Tests** for steps 1 to 4: a test setup with its own Kanidm checks that a visitor sees
only the stripped-down list, that logging in works, that a reader can't act, that an admin
can't act from the internet, and that a faked "came over Tailscale" from anywhere but the
router is refused.

## Step 5: The homelab side

In `~/.flake`:

- Kanidm gets a Lodestar connection and the group `lodestar-users` (you). Claude copies its
  secret into 1Password (**you** approve the Touch ID prompt), and nixos-manager reads it
  from there.
- nixos-manager opens the new entrance for the router only.
- The router gets `lodestar.vanwaaij.com`, public and over Tailscale.
- The Lodestar link and the Kanidm "Services" tile point to `https://lodestar.vanwaaij.com`.

## Step 6: Tailscale (you)

Create the Service `lodestar` in the Tailscale admin console. Claude does the policy and
checks that the name works over Tailscale.

## Step 7: Trying it

Claude checks it from the internet and over Tailscale first. Then **you** try:

1. Logged out, on mobile data: only the Services list, no links.
2. Logged in on mobile data: the whole dashboard, buttons greyed out.
3. Logged in over Tailscale: the buttons work.

**If anything is wrong:** the router route goes off with one change; the old tailnet
address keeps working throughout.

## Step 8: Paperwork

Claude updates the docs in both repos: who sees what, the groups, how to give someone read
access, and where the emergency way in is (the tailnet address and the CLI).

## When this is done

- `lodestar.vanwaaij.com` shows the public overview of what you host.
- Logging in shows everything; acting needs Tailscale.
- Giving someone read access is adding their name to `lodestar-users` in the repo.
