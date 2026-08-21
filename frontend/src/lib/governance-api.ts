import { api } from "./api/api";
import type {
  Proposal,
  ProposalsListResponse,
  CreateProposalRequest,
  CastVoteRequest,
  Vote,
  Comment,
  ProposalStatus,
} from "@/types/governance";

export async function getProposals(
  status?: ProposalStatus,
  limit?: number,
  offset?: number,
): Promise<ProposalsListResponse> {
  const params = new URLSearchParams();
  if (status) params.append("status", status);
  if (limit !== undefined) params.append("limit", limit.toString());
  if (offset !== undefined) params.append("offset", offset.toString());
  const query = params.toString();
  return api.get<ProposalsListResponse>(
    `/governance/proposals${query ? `?${query}` : ""}`,
  );
}

export async function getProposal(id: string): Promise<Proposal> {
  return api.get<Proposal>(`/governance/proposals/${id}`);
}

export interface GovernanceInsightsResponse {
  insights: string[];
  /**
   * ISO timestamp of when these insights were generated. Governance data
   * (backend#14) may currently be sourced via storage polling rather than
   * contract events until the governance/governance-voting contracts add
   * them (contracts#10/#12), so this can lag noticeably behind other
   * insight callouts — surface it in the UI rather than hiding the lag.
   */
  generated_at?: string;
}

/**
 * Auto-generated governance insights: proposal momentum, voter turnout, etc.
 * Returns an empty list rather than throwing so the callout degrades
 * gracefully if the endpoint isn't available yet.
 */
export async function getGovernanceInsights(): Promise<GovernanceInsightsResponse> {
  return api.get<GovernanceInsightsResponse>("/governance/insights");
}

export async function createProposal(
  request: CreateProposalRequest,
  authToken: string,
): Promise<Proposal> {
  return api.post<Proposal>("/governance/proposals", request, {
    headers: { Authorization: `Bearer ${authToken}` },
  });
}

export async function castVote(
  proposalId: string,
  request: CastVoteRequest,
  authToken: string,
): Promise<Vote> {
  return api.post<Vote>(
    `/governance/proposals/${proposalId}/vote`,
    request,
    { headers: { Authorization: `Bearer ${authToken}` } },
  );
}

export async function getVotes(proposalId: string): Promise<Vote[]> {
  return api.get<Vote[]>(`/governance/proposals/${proposalId}/votes`);
}

export async function hasVoted(
  proposalId: string,
  address: string,
): Promise<{ has_voted: boolean }> {
  return api.get<{ has_voted: boolean }>(
    `/governance/proposals/${proposalId}/has-voted/${address}`,
  );
}

export async function addComment(
  proposalId: string,
  content: string,
  authToken: string,
): Promise<Comment> {
  return api.post<Comment>(
    `/governance/proposals/${proposalId}/comments`,
    { content },
    { headers: { Authorization: `Bearer ${authToken}` } },
  );
}

export async function getComments(proposalId: string): Promise<Comment[]> {
  return api.get<Comment[]>(`/governance/proposals/${proposalId}/comments`);
}
