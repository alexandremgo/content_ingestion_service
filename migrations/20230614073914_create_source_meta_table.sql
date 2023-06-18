-- Create SourceMeta Table
-- timestamptz is a time-zone aware date and time type
CREATE TYPE source_type AS ENUM ('epub');

-- TODO: user_id foreign key
-- TODO: object_store_name as a Varchar ? With the 4 "-" symbols, UUID v4 is 36 characters long
CREATE TABLE source_meta(
   id uuid NOT NULL,
   PRIMARY KEY (id),
   user_id uuid NOT NULL,
   object_store_name VARCHAR(60) NOT NULL UNIQUE,
   source_type source_type NOT NULL,
   initial_name TEXT NOT NULL,
   added_at timestamptz NOT NULL,
   extracted_at timestamptz
);
