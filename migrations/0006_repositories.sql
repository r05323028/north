CREATE TABLE repositories (
    id TEXT PRIMARY KEY
        CHECK (id = LOWER(id)
            AND id ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'),
    name TEXT NOT NULL
        CHECK (OCTET_LENGTH(BTRIM(name)) BETWEEN 1 AND 100),
    name_normalized TEXT NOT NULL UNIQUE
        CHECK (CHAR_LENGTH(BTRIM(name_normalized)) BETWEEN 1 AND 400),
    url TEXT NOT NULL
        CHECK (OCTET_LENGTH(BTRIM(url)) BETWEEN 1 AND 2048),
    description TEXT NOT NULL DEFAULT ''
        CHECK (OCTET_LENGTH(BTRIM(description)) BETWEEN 0 AND 10000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    disabled_at TIMESTAMPTZ
);

CREATE INDEX repositories_catalog_order_idx
ON repositories (name_normalized ASC, id ASC);

CREATE FUNCTION PREVENT_REPOSITORY_URL_MUTATION()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
BEGIN
    IF OLD.id IS DISTINCT FROM NEW.id
        OR OLD.url IS DISTINCT FROM NEW.url
        OR OLD.created_at IS DISTINCT FROM NEW.created_at THEN
        RAISE EXCEPTION 'repository identity, URL, and created_at are immutable';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER repositories_url_immutable
BEFORE UPDATE ON repositories
FOR EACH ROW EXECUTE FUNCTION PREVENT_REPOSITORY_URL_MUTATION();
