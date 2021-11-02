-- SQLite >=3.35.0 supports DROP COLUMN.
CREATE TEMPORARY TABLE persons_backup (id, name, age);
INSERT INTO persons_backup SELECT id, name, age FROM persons;
DROP TABLE persons;
CREATE TABLE persons (
	 id INTEGER PRIMARY KEY NOT NULL
	,name TEXT NOT NULL
	,age INTEGER NOT NULL
);
INSERT INTO persons SELECT id, name, age FROM persons_backup;
DROP TABLE persons_backup;
