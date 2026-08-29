export type Kind = "fact" | "inference";
export type EntityType = "person" | "organization" | "engagement" | "interaction" | "entity";
export type RefTargetType = EntityType | "fact";
export type DetailType = "person" | "organization" | "engagement";
export type DetailTarget = { type: DetailType; id: number };
export type OpenDetail = (type: DetailType, id: number) => void;

export type Alias = { alias: string; kind?: string };

export type PersonSummary = {
  id: number;
  name: string;
  aliases: Alias[];
  org_id?: number;
  org_name?: string;
  role?: string;
  first_met?: string;
  last_seen?: string;
};

export type OrganizationSummary = {
  id: number;
  name: string;
  kind?: string;
};

export type EngagementSummary = {
  id: number;
  name: string;
  scope: string;
  org_id?: number;
  org_name?: string;
  status?: string;
  started_at?: string;
  ended_at?: string;
};

export type EngagementPerson = { person: PersonSummary; role?: string };

export type Fact = {
  id: number;
  entity_type: EntityType;
  entity_id: number;
  statement: string;
  kind: Kind;
  scope: string;
  created_at: string;
  predicate?: string;
  value?: string;
  valid_from?: string;
  superseded_by?: number;
};

export type Reference = {
  id: number;
  target_type: RefTargetType;
  target_id: number;
  system: string;
  uri: string;
  note: string;
  scope: string;
  created_at: string;
  title?: string;
  snapshot?: string;
  last_verified?: string;
};

export type GlossaryTerm = {
  id: number;
  term: string;
  scope: string;
  reading?: string;
  definition?: string;
  engagement_id?: number;
};

export type InteractionSummary = {
  id: number;
  kind: string;
  occurred_at: string;
  summary: string;
  scope: string;
  person_ids: number[];
  engagement_id?: number;
};

export type SearchEntity = {
  type: EntityType;
  id: number;
  name: string;
  summary: string;
  score: number;
  matched_on: string[];
  facts: Fact[];
  refs: Reference[];
};

export type SearchContextOutput = {
  query: string;
  scopes: string[];
  cross_scope: boolean;
  entities: SearchEntity[];
  glossary: GlossaryTerm[];
  interactions: InteractionSummary[];
  hints: string[];
};

export type GetPersonOutput = {
  person: PersonSummary;
  organization?: OrganizationSummary;
  engagements: EngagementSummary[];
  facts: Fact[];
  refs: Reference[];
  interactions: InteractionSummary[];
};

export type GetOrganizationOutput = {
  organization: OrganizationSummary;
  people: PersonSummary[];
  engagements: EngagementSummary[];
  facts: Fact[];
  refs: Reference[];
};

export type GetEngagementOutput = {
  engagement: EngagementSummary;
  organization?: OrganizationSummary;
  people: EngagementPerson[];
  facts: Fact[];
  refs: Reference[];
  glossary: GlossaryTerm[];
  interactions: InteractionSummary[];
};

export type DetailResult =
  | { type: "person"; data: GetPersonOutput }
  | { type: "organization"; data: GetOrganizationOutput }
  | { type: "engagement"; data: GetEngagementOutput };

export type ResolveSourceOutput = {
  reference: Reference;
  resolved: boolean;
  content?: string;
  reason?: string;
};
