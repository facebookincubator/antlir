BEGIN;

CREATE TABLE runs(
  revision char(40) NOT NULL PRIMARY KEY,
  time timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE targets(
  target varchar(260) NOT NULL PRIMARY KEY CHECK (target <> '')
);

CREATE TABLE tests(
  target varchar(260) NOT NULL REFERENCES targets (target) ON DELETE CASCADE,
  test varchar(260) NOT NULL,
  disabled boolean NOT NULL DEFAULT false,
  PRIMARY KEY (target, test)
);

CREATE INDEX tests_disabled_idx ON tests (disabled);

CREATE TABLE results(
  revision char(40) NOT NULL REFERENCES runs (revision) ON DELETE CASCADE,
  target varchar(260) NOT NULL,
  test varchar(260) NOT NULL,
  passed boolean NOT NULL,
  PRIMARY KEY (revision, target, test),
  FOREIGN KEY (target, test) REFERENCES tests (target, test) ON DELETE CASCADE
);

COMMIT;
