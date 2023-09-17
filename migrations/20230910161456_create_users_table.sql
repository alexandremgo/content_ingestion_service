-- Create the `users` table

CREATE TABLE users(
   id uuid PRIMARY KEY,
   email TEXT NOT NULL UNIQUE,
   password_hash TEXT NOT NULL,
   created_at timestamptz NOT NULL,
   updated_at timestamptz NOT NULL
);
