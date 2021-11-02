ALTER TABLE persons ADD COLUMN state TEXT NOT NULL DEFAULT "";
UPDATE persons SET state = "New York" WHERE name = "Alice";
UPDATE persons SET state = "New Hampshire" WHERE name = "Bob";
UPDATE persons SET state = "Maryland" WHERE name = "Eve";
