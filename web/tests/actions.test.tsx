// @vitest-environment jsdom
// Actions from the page: the buttons call the right action with the right arguments, and
// are disabled when they can't act.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChannelView } from "@/bindings/ChannelView";
import type { HostView } from "@/bindings/HostView";
import { ChannelCard } from "@/components/fleet/ChannelCard";
import { HostRow } from "@/components/fleet/HostRow";
import { PromoteDialog } from "@/components/fleet/PromoteCard";
import { apply, fakeActions, host, NOW, REV, withActions } from "./helpers";

function renderRow(h: HostView, actions = fakeActions()) {
  const r = render(
    withActions(
      actions,
      <table>
        <tbody>
          <HostRow host={h} now={NOW} channelRole="unstable" />
        </tbody>
      </table>,
    ),
  );
  return { ...r, actions };
}

afterEach(cleanup);

describe("HostRow actions", () => {

  it("opens the log of the last apply from an icon button with a label", () => {
    const { actions } = renderRow(host({ last_apply: apply({ apply_id: "a7" }) }));
    fireEvent.click(screen.getByRole("button", { name: "Show log for desktop" }));
    expect(actions.showLog).toHaveBeenCalledWith("desktop", "a7");
  });

  it("has no log button before the first apply", () => {
    renderRow(host({ last_apply: null }));
    expect(screen.queryByRole("button", { name: /Show log/ })).toBeNull();
    expect(screen.getByText("Never applied")).toBeTruthy();
  });

  it("applies newest on click, and not while an apply runs", async () => {
    const { actions } = renderRow(host({ behind: 2 }));
    fireEvent.click(screen.getByRole("button", { name: "Apply newest" }));
    expect(actions.applyHost).toHaveBeenCalledWith("desktop");
    cleanup();
    renderRow(host({ last_apply: apply({ state: "running", finished_at: null }) }));
    const b = screen.getByRole("button", { name: "Applying" }) as HTMLButtonElement;
    expect(b.disabled).toBe(true);
  });

  it("says when an apply is queued for a machine that is off", () => {
    renderRow(host({ online: "off", desired_apply_id: "a9", last_apply: apply({ apply_id: "a1" }) }));
    expect(screen.getByText("Queued: sent when desktop comes online")).toBeTruthy();
  });
});

describe("ChannelCard", () => {
  const channel = (over: Partial<ChannelView>): ChannelView => ({
    name: "unstable",
    head: REV,
    head_subject: "fix(users): linger",
    ...over,
  });

  it("shows the head and how many machines are behind; applies to the channel", () => {
    const actions = fakeActions();
    const { container } = render(
      withActions(
        actions,
        <ChannelCard
          channel={channel({})}
          hosts={[host({ name: "a", behind: 0 }), host({ name: "b", behind: 2 }), host({ name: "c", behind: null })]}
          role="unstable"
        />,
      ),
    );
    expect(container.textContent).toContain("3 machines · 1 behind");
    expect(screen.getByText("0123456")).toBeTruthy();
    expect(screen.getByText("fix(users): linger")).toBeTruthy();
    expect(container.querySelector("[data-role=unstable]")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Apply newest to all on unstable" }));
    expect(actions.applyChannel).toHaveBeenCalledWith("unstable");
  });

  it("can't apply without a head or without machines", () => {
    render(withActions(fakeActions(), <ChannelCard channel={channel({ head: null })} hosts={[host({})]} role="stable" />));
    expect(screen.getByText("No head: branch not in the mirror")).toBeTruthy();
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
    cleanup();
    render(withActions(fakeActions(), <ChannelCard channel={channel({})} hosts={[]} role="stable" />));
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("PromoteDialog", () => {
  it("promotes the commit it showed when it opened, even if the candidate moved since", async () => {
    const actions = fakeActions();
    const pair = { from: "unstable", to: "stable" };
    const channels: ChannelView[] = [
      { name: "unstable", head: REV, head_subject: "first" },
      { name: "stable", head: "f".repeat(40), head_subject: "old" },
    ];
    const ui = (candidate: string) =>
      withActions(actions, <PromoteDialog candidate={candidate} pair={pair} channels={channels} />);
    const { rerender } = render(ui(REV));
    fireEvent.click(screen.getByRole("button", { name: "Promote 0123456" }));
    expect(await screen.findByRole("dialog")).toBeTruthy();
    rerender(ui("a".repeat(40)));
    fireEvent.click(screen.getAllByRole("button", { name: "Promote 0123456" }).at(-1)!);
    await vi.waitFor(() => expect(actions.promote).toHaveBeenCalledWith(REV, pair));
  });
});
