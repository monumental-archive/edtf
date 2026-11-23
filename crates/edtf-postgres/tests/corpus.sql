-- Shared SQL conformance corpus for edtf_postgres.
--
-- Run by BOTH paths, so the binary is held to the same standard as the
-- source and the two cannot drift:
--   * the pgrx suite, via Spi::run in src/lib.rs
--   * the prebuilt-tarball smoke test, via psql -f, against a real install
--
-- Deliberately not a second, weaker set of assertions written for the smoke
-- test, and not mirrored by Rust tests in src/lib.rs: a copy would drift and
-- rot within two releases; this is the one source of truth for "does this
-- extension work". A new SQL assertion is a row here, never a new pgrx test.
--
-- Every check is a plpgsql ASSERT, so the first failure aborts with the
-- message and a non-zero exit under `psql -v ON_ERROR_STOP=1`.
--
-- NOT in ./sql/ on purpose: pgrx copies ./sql/*.sql into the install tree as
-- extension scripts. This is a test fixture, not part of the extension.

-- Assertions are on by default; a corpus that silently passed because the
-- default had been changed would be worse than no corpus.
--
-- set_config(..., false) is the session-level equivalent of SET, written as a
-- function call because sqlfluff's postgres dialect cannot parse a namespaced
-- GUC assignment (`SET plpgsql.check_asserts = on;`), and one unparsable
-- statement drops the whole file from lint:sql.
SELECT set_config('plpgsql.check_asserts', 'on', false);

DO $corpus$
DECLARE
    failures text;
BEGIN
    -- edtf_valid ---------------------------------------------------------
    ASSERT edtf_valid('1985-04-12'), 'level 0 calendar date';
    ASSERT edtf_valid('2004-06~-11'), 'level 2 qualified month';
    ASSERT edtf_valid('{1667,1668,1670..1672}'), 'level 2 set with range';
    ASSERT edtf_valid('1985-24'), 'level 1 season (winter 1985)';
    ASSERT NOT edtf_valid('1985-02-30'), 'no such calendar day';
    ASSERT NOT edtf_valid('19850412'), 'basic format is not EDTF';
    ASSERT NOT edtf_valid('2004/2003'), 'interval must not run backwards';

    -- edtf_level ---------------------------------------------------------
    ASSERT edtf_level('1985-04-12') = 0, 'level 0';
    ASSERT edtf_level('1985~') = 1, 'level 1';
    ASSERT edtf_level('156X-12-25') = 2, 'level 2';
    ASSERT edtf_level('junk') IS NULL, 'level of an invalid expression is NULL';

    -- edtf_canonical -----------------------------------------------------
    ASSERT edtf_canonical('?2004-?06-?11') = '2004-06-11?',
        'per-component qualification collapses to whole-expression form';
    ASSERT edtf_canonical('junk') IS NULL, 'canonical of an invalid expression is NULL';

    -- edtf_min / edtf_max ------------------------------------------------
    ASSERT edtf_min('1985')::text = '1985-01-01', 'year floor';
    ASSERT edtf_max('1985')::text = '1985-12-31', 'year ceiling';
    ASSERT edtf_min('156X')::text = '1560-01-01', 'unspecified-digit decade floor';
    ASSERT edtf_max('156X')::text = '1569-12-31', 'unspecified-digit decade ceiling';
    -- Northern winter wraps into the following (leap) year.
    ASSERT edtf_max('2003-24')::text = '2004-02-29', 'season 24 wraps across the year boundary';
    ASSERT edtf_max('1985-04-12/..')::text = 'infinity', 'open interval end';
    ASSERT edtf_min('../1985')::text = '-infinity', 'open interval start';
    -- Bounds convert as a day count (the Julian Day Number), so EDTF's
    -- astronomical years land on Postgres's BC numbering: year 0000 is
    -- 1 BC (a leap year), -0001 is 2 BC, -4712 is 4713 BC.
    ASSERT edtf_min('-4712')::text = '4713-01-01 BC', 'astronomical -4712 is 4713 BC';
    ASSERT edtf_max('-0001')::text = '0002-12-31 BC', 'astronomical -0001 is 2 BC';
    ASSERT edtf_min('0000')::text = '0001-01-01 BC', 'year 0000 is 1 BC: floor';
    ASSERT edtf_max('0000')::text = '0001-12-31 BC', 'year 0000 is 1 BC: ceiling';
    ASSERT edtf_max('0000-02')::text = '0001-02-29 BC', 'year 0000 is a leap year';
    ASSERT edtf_min('0001')::text = '0001-01-01', 'year 0001 is AD 1';
    -- Postgres holds 4714-11-24 BC (Julian day 0) to 5874897-12-31. A day
    -- outside that range saturates to -infinity / infinity, which is not a
    -- date and is never a silently clamped one: it still compares and
    -- contains correctly against every date Postgres can hold. NULL means
    -- only that the bound is unknown.
    ASSERT edtf_min('-4713-11-24')::text = '4714-11-24 BC', 'the Postgres floor';
    ASSERT edtf_min('-4713-11-23')::text = '-infinity', 'the day before the floor saturates';
    ASSERT edtf_min('-4713')::text = '-infinity', 'a year starting before the floor saturates';
    ASSERT edtf_max('-4713')::text = '4714-12-31 BC', 'that year still ends inside the range';
    ASSERT edtf_max('Y5874897')::text = '5874897-12-31', 'the Postgres ceiling';
    ASSERT edtf_min('Y5874898')::text = 'infinity', 'the year after the ceiling saturates';
    ASSERT edtf_min('Y-17E7')::text = '-infinity', 'deep past: earliest saturates';
    ASSERT edtf_max('Y-17E7')::text = '-infinity', 'deep past: latest saturates';
    ASSERT edtf_max('Y17E7')::text = 'infinity', 'deep future: latest saturates';
    ASSERT NOT (daterange(edtf_min('Y-17E7'), edtf_max('Y-17E7'), '[]') @> DATE '1965-06-15'),
        'a saturated deep-past range does not contain a real date';
    ASSERT edtf_max('1986-04/') IS NULL, 'unknown interval end is NULL, not infinity';

    -- Sweeps against Postgres's own calendar, which shares no code with
    -- edtf-core: #304 shifted every year at or below 0 by a day, and no
    -- example above had caught it. The EDTF spelling is built from the
    -- date's text, never to_char or extract, which fail outside the
    -- timestamp range. A failure names its first five inputs.
    --
    -- Every 211th day from the Postgres floor to 9999-12-31 (25,467
    -- days), spelled with its astronomical year, bounds to itself.
    SELECT string_agg(
        spelled || ' -> ' || coalesce(edtf_min(spelled)::text, 'NULL')
        || ' .. ' || coalesce(edtf_max(spelled)::text, 'NULL'), '; '
    )
    INTO failures
    FROM (
        SELECT spelled
        FROM (
            SELECT
                day,
                CASE WHEN astro < 0
                        THEN '-' || lpad((-astro)::text, 4, '0')
                    ELSE lpad(astro::text, 4, '0')
                END || substr(day::text, 5, 6) AS spelled
            FROM (
                SELECT
                    day,
                    CASE WHEN day::text LIKE '% BC'
                            THEN 1 - split_part(day::text, '-', 1)::int
                        ELSE split_part(day::text, '-', 1)::int
                    END AS astro
                FROM (
                    SELECT DATE '4714-11-24 BC' + offs AS day
                    FROM generate_series(
                        0, DATE '9999-12-31' - DATE '4714-11-24 BC', 211
                    ) AS offs
                ) AS days
            ) AS years
        ) AS spellings
        WHERE
            edtf_min(spelled) IS DISTINCT FROM day
            OR edtf_max(spelled) IS DISTINCT FROM day
        ORDER BY day
        LIMIT 5
    ) AS first_failures;
    ASSERT failures IS NULL, 'day sweep, day does not bound to itself: ' || failures;

    -- Every 7th year from -4712 to 9999, every 1009th from 10000 to the
    -- Postgres ceiling, and the ceiling itself (7,916 years), bounds to
    -- its 1 January and 31 December. make_date numbers BC years
    -- negatively, so astronomical year y <= 0 is make_date(y - 1, ...).
    SELECT string_agg(
        spelled || ' -> ' || coalesce(edtf_min(spelled)::text, 'NULL')
        || ' .. ' || coalesce(edtf_max(spelled)::text, 'NULL'), '; '
    )
    INTO failures
    FROM (
        SELECT spelled
        FROM (
            SELECT
                yr,
                CASE
                    WHEN yr > 9999 THEN 'Y' || yr
                    WHEN yr < 0 THEN '-' || lpad((-yr)::text, 4, '0')
                    ELSE lpad(yr::text, 4, '0')
                END AS spelled,
                CASE WHEN yr <= 0 THEN yr - 1 ELSE yr END AS pg_year
            FROM (
                SELECT generate_series(-4712, 9999, 7) AS yr
                UNION ALL
                SELECT generate_series(10000, 5874897, 1009)
                UNION ALL
                SELECT 5874897
            ) AS years
        ) AS spellings
        WHERE
            edtf_min(spelled) IS DISTINCT FROM make_date(pg_year, 1, 1)
            OR edtf_max(spelled) IS DISTINCT FROM make_date(pg_year, 12, 31)
        ORDER BY yr
        LIMIT 5
    ) AS first_failures;
    ASSERT failures IS NULL, 'year sweep, year does not bound to its first and last day: '
        || failures;

    -- edtf_relation ------------------------------------------------------
    ASSERT edtf_relation('1985~', '199X') = ARRAY['definitely_before'],
        'approximation never moves bounds, so the relation stays definite';
    ASSERT edtf_relation('198X', '1985') = ARRAY[
        'possibly_before', 'possibly_after', 'possibly_overlaps',
        'possibly_contains', 'possibly_within', 'possibly_equal'
    ], 'overlapping uncertainty yields all six as possible, none definite';
    -- An unknown interval end must never produce a definite verdict.
    ASSERT NOT ('definitely_after' = ANY(edtf_relation('1985/', '../1980'))),
        'unknown ends are never definite';
    -- The documented consistency-rule shape: born must not be after died.
    ASSERT NOT ('definitely_after' = ANY(edtf_relation('1890~', '1976-01-12'))),
        'consistency rule: birth before death';
    ASSERT edtf_relation('junk', '1985') IS NULL, 'relation with an invalid operand is NULL';

    -- Intended usage: index-friendly range overlap ------------------------
    -- Bounds pinned first, deliberately: daterange(NULL, NULL, '[]') is
    -- UNBOUNDED and therefore contains every date, so the containment
    -- assertions below would pass even if both functions returned NULL.
    ASSERT edtf_min('196X')::text = '1960-01-01', 'decade floor';
    ASSERT edtf_max('196X')::text = '1969-12-31', 'decade ceiling';
    ASSERT NOT (daterange(edtf_min('156X'), edtf_max('156X'), '[]') @> DATE '1965-06-15'),
        '1965 is outside the 1560s';
    ASSERT daterange(edtf_min('196X'), edtf_max('196X'), '[]') @> DATE '1965-06-15',
        '1965 is inside the 1960s';
END
$corpus$;
