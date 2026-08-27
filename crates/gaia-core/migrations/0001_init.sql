-- gaia-library DDL v1（仕様書 §5.1）。名寄せ層は共有、内容層は scope 必須。
-- 名寄せ層（共有・scope なし）
CREATE TABLE affiliations (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  identity   TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE organizations (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  kind       TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE people (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  org_id     INTEGER REFERENCES organizations(id),
  role       TEXT,
  first_met  TEXT,
  last_seen  TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE person_aliases (
  person_id  INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
  alias      TEXT NOT NULL,
  kind       TEXT,
  PRIMARY KEY (person_id, alias)
);
CREATE TABLE entities (
  id         INTEGER PRIMARY KEY,
  type       TEXT NOT NULL,
  name       TEXT NOT NULL,
  attrs      TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 内容層（scope 必須）
CREATE TABLE engagements (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  org_id     INTEGER REFERENCES organizations(id),
  scope      TEXT NOT NULL REFERENCES affiliations(name),
  status     TEXT,
  started_at TEXT,
  ended_at   TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE engagement_people (
  engagement_id INTEGER NOT NULL REFERENCES engagements(id) ON DELETE CASCADE,
  person_id     INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
  role          TEXT,
  PRIMARY KEY (engagement_id, person_id)
);
CREATE TABLE interactions (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL,
  occurred_at   TEXT NOT NULL,
  summary       TEXT NOT NULL,
  engagement_id INTEGER REFERENCES engagements(id),
  scope         TEXT NOT NULL REFERENCES affiliations(name),
  created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE interaction_people (
  interaction_id INTEGER NOT NULL REFERENCES interactions(id) ON DELETE CASCADE,
  person_id      INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
  PRIMARY KEY (interaction_id, person_id)
);
CREATE TABLE facts (
  id            INTEGER PRIMARY KEY,
  entity_type   TEXT NOT NULL CHECK (entity_type IN ('person','organization','engagement','interaction','entity')),
  entity_id     INTEGER NOT NULL,
  statement     TEXT NOT NULL,
  predicate     TEXT,
  value         TEXT,
  kind          TEXT NOT NULL CHECK (kind IN ('fact','inference')),
  scope         TEXT NOT NULL REFERENCES affiliations(name),
  valid_from    TEXT,
  superseded_by INTEGER REFERENCES facts(id),
  created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE refs (
  id            INTEGER PRIMARY KEY,
  target_type   TEXT NOT NULL CHECK (target_type IN ('person','organization','engagement','interaction','entity','fact')),
  target_id     INTEGER NOT NULL,
  system        TEXT NOT NULL,
  uri           TEXT NOT NULL,
  title         TEXT,
  note          TEXT NOT NULL,
  snapshot      TEXT,
  scope         TEXT NOT NULL REFERENCES affiliations(name),
  last_verified TEXT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE glossary (
  id            INTEGER PRIMARY KEY,
  engagement_id INTEGER REFERENCES engagements(id),
  term          TEXT NOT NULL,
  reading       TEXT,
  definition    TEXT,
  scope         TEXT NOT NULL REFERENCES affiliations(name),
  created_at    TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE proposals (
  id            INTEGER PRIMARY KEY,
  action        TEXT NOT NULL CHECK (action IN ('insert','update','supersede')),
  target_type   TEXT NOT NULL,
  target_id     INTEGER,
  patch         TEXT NOT NULL,
  kind          TEXT NOT NULL CHECK (kind IN ('fact','inference')),
  scope         TEXT NOT NULL REFERENCES affiliations(name),
  provenance    TEXT,
  provenance_id INTEGER REFERENCES refs(id),
  proposed_by   TEXT NOT NULL,
  request_id    TEXT NOT NULL UNIQUE,
  status        TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected')),
  result_id     INTEGER,
  decision_note TEXT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now')),
  decided_at    TEXT,
  decided_by    TEXT
);
CREATE TABLE audit_log (
  id     INTEGER PRIMARY KEY,
  actor  TEXT NOT NULL,
  action TEXT NOT NULL,
  detail TEXT NOT NULL,
  at     TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_facts_target ON facts(entity_type, entity_id);
CREATE INDEX idx_refs_target  ON refs(target_type, target_id);
CREATE INDEX idx_facts_scope  ON facts(scope);
CREATE INDEX idx_refs_scope   ON refs(scope);
CREATE INDEX idx_alias_lookup ON person_aliases(alias);
CREATE INDEX idx_proposals_status ON proposals(status, scope);

-- 外部コンテンツ FTS（trigram）と同期トリガ
CREATE VIRTUAL TABLE facts_fts USING fts5(statement, content='facts', content_rowid='id', tokenize='trigram');
CREATE TRIGGER facts_ai AFTER INSERT ON facts BEGIN
  INSERT INTO facts_fts(rowid, statement) VALUES (new.id, new.statement);
END;
CREATE TRIGGER facts_ad AFTER DELETE ON facts BEGIN
  INSERT INTO facts_fts(facts_fts, rowid, statement) VALUES ('delete', old.id, old.statement);
END;
CREATE TRIGGER facts_au AFTER UPDATE OF statement ON facts BEGIN
  INSERT INTO facts_fts(facts_fts, rowid, statement) VALUES ('delete', old.id, old.statement);
  INSERT INTO facts_fts(rowid, statement) VALUES (new.id, new.statement);
END;
