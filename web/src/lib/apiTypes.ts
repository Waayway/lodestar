// Server answers that ts-rs does not generate (they are built ad hoc in the server's api).
// Shared by connection/client (which receives them) and lib/format (which words them).

/** What the server got back from the agent when it sent an apply. */
export type AgentStatus = "202" | "409" | "200" | "unreachable" | "error";

export type ApplyHostResponse = { apply_id: string; agent_status: AgentStatus };

export type ApplyChannelResponse = {
  apply_ids: Record<string, string>;
  agent_status: Record<string, AgentStatus>;
};
