"""Live verification matrix: official Snowflake connector → chukei → real account.

Driven by scripts/live-pilot.sh. Connection comes from env:
  SNOWFLAKE_ACCOUNT / SNOWFLAKE_USER / SNOWFLAKE_PASSWORD / SNOWFLAKE_WAREHOUSE
  PILOT_TEST_HOST / PILOT_TEST_PORT / PILOT_TEST_PROTOCOL (http|https)
  PILOT_TEST_AUTH (password|keypair|pat), PILOT_TEST_KEY (keypair pem path),
  PILOT_TEST_PAT (token value, pat mode)

Usage: live_matrix.py STAGE [STAGE...]   stages: core shapes concurrency
Every stage prints "  ✓ <check>" lines; any assertion failure exits non-zero.
"""

import concurrent.futures
import os
import sys
import tempfile
import time

import snowflake.connector


def ok(name):
    print(f"  ✓ {name}", flush=True)


def connect(**overrides):
    auth = os.environ.get("PILOT_TEST_AUTH", "password")
    kw = dict(
        user=os.environ["SNOWFLAKE_USER"],
        account=os.environ["SNOWFLAKE_ACCOUNT"],
        warehouse=os.environ.get("SNOWFLAKE_WAREHOUSE", "COMPUTE_WH"),
        host=os.environ.get("PILOT_TEST_HOST", "127.0.0.1"),
        port=int(os.environ.get("PILOT_TEST_PORT", "18920")),
        protocol=os.environ.get("PILOT_TEST_PROTOCOL", "http"),
    )
    if auth == "password":
        kw["password"] = os.environ["SNOWFLAKE_PASSWORD"]
    elif auth == "keypair":
        from cryptography.hazmat.primitives import serialization

        with open(os.environ["PILOT_TEST_KEY"], "rb") as f:
            pkey = serialization.load_pem_private_key(f.read(), password=None)
        kw["private_key"] = pkey.private_bytes(
            encoding=serialization.Encoding.DER,
            format=serialization.PrivateFormat.PKCS8,
            encryption_algorithm=serialization.NoEncryption(),
        )
    elif auth == "pat":
        # AuthByPAT reads the `token` param (password= is silently ignored)
        kw["token"] = os.environ["PILOT_TEST_PAT"]
        kw["authenticator"] = "PROGRAMMATIC_ACCESS_TOKEN"
    elif auth == "externalbrowser":
        # Interactive: a browser window opens against Snowflake directly;
        # only the token exchange transits chukei. Requires a human.
        kw["authenticator"] = "externalbrowser"
    else:
        raise SystemExit(f"unknown PILOT_TEST_AUTH={auth}")
    kw.update(overrides)
    return snowflake.connector.connect(**kw)


def stage_core(conn):
    cur = conn.cursor()
    user, wh, ver = cur.execute(
        "SELECT CURRENT_USER(), CURRENT_WAREHOUSE(), CURRENT_VERSION()"
    ).fetchone()
    assert user.upper() == os.environ["SNOWFLAKE_USER"].upper(), (user, ver)
    auth = os.environ.get("PILOT_TEST_AUTH", "password")
    proto = os.environ.get("PILOT_TEST_PROTOCOL", "http")
    ok(f"login + session [auth={auth} transport={proto}] (Snowflake {ver}, wh {wh})")

    cur.execute("CREATE DATABASE IF NOT EXISTS CHUKEI_PILOT")
    cur.execute("USE SCHEMA CHUKEI_PILOT.PUBLIC")
    cur.execute("CREATE OR REPLACE TABLE t (region VARCHAR, amount INT)")
    cur.execute("INSERT INTO t VALUES ('emea', 10), ('apac', 20), ('amer', 30)")

    q = "SELECT region, amount FROM t WHERE region = 'emea' OR region = 'apac' ORDER BY amount"
    rows = cur.execute(q).fetchall()
    assert rows == [("emea", 10), ("apac", 20)], rows
    assert cur.execute(q).fetchall() == rows
    ok("repeat read returns identical rows (cache path)")

    rows2 = cur.execute(
        "SELECT region, amount FROM t WHERE region = 'amer' OR region = 'apac' ORDER BY amount"
    ).fetchall()
    assert rows2 == [("apac", 20), ("amer", 30)], rows2
    ok("different literals do not reuse the first entry")

    r1 = cur.execute("SELECT RANDOM()").fetchone()[0]
    time.sleep(0.2)
    r2 = cur.execute("SELECT RANDOM()").fetchone()[0]
    assert r1 != r2, "RANDOM() served from cache — determinism gate FAILED"
    ok("non-deterministic query never cached")

    c1 = cur.execute("SELECT COUNT(*) FROM t").fetchone()[0]
    cur.execute("INSERT INTO t VALUES ('latam', 40)")
    c2 = cur.execute("SELECT COUNT(*) FROM t").fetchone()[0]
    assert (c1, c2) == (3, 4), f"stale read after write: {c1} → {c2}"
    ok("write invalidates cached COUNT")

    big = cur.execute("SELECT seq4() s FROM TABLE(GENERATOR(ROWCOUNT => 200000))")
    got = sum(len(b) for b in iter(lambda: big.fetchmany(50000), []))
    assert got == 200000, f"chunked result truncated: {got}"
    ok("200k-row (chunked) result passes through intact")


def stage_shapes(conn):
    """Statement shapes that must fail open to passthrough, live."""
    cur = conn.cursor()
    cur.execute("USE SCHEMA CHUKEI_PILOT.PUBLIC")

    rows = cur.execute("SHOW WAREHOUSES").fetchall()
    assert rows, "SHOW WAREHOUSES returned nothing"
    cur.execute("DESCRIBE TABLE t")
    cur.execute("USE WAREHOUSE " + os.environ.get("SNOWFLAKE_WAREHOUSE", "COMPUTE_WH"))
    ok("SHOW / DESCRIBE / USE round-trip")

    # PUT/GET through a stage — exercises the file-transfer response shape
    # (presigned URL JSON) which chukei must pass through untouched.
    cur.execute("CREATE STAGE IF NOT EXISTS pilot_stage")
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "pilot_upload.csv")
        with open(src, "w") as f:
            f.write("a,1\nb,2\n")
        cur.execute(f"PUT file://{src} @pilot_stage AUTO_COMPRESS=FALSE OVERWRITE=TRUE")
        listed = cur.execute("LIST @pilot_stage").fetchall()
        assert any("pilot_upload.csv" in r[0] for r in listed), listed
        cur.execute(f"GET @pilot_stage/pilot_upload.csv file://{d}/")
        fetched = os.path.join(d, "pilot_upload.csv")
        assert open(fetched).read() == "a,1\nb,2\n"
    ok("PUT + LIST + GET via stage (file-transfer passthrough)")

    # A statement sqlparser-rs cannot parse must still execute (fail-open).
    # MATCH_RECOGNIZE is Snowflake-specific syntax outside the dialect.
    rows = cur.execute(
        """
        SELECT * FROM t MATCH_RECOGNIZE(
          ORDER BY amount
          MEASURES MATCH_NUMBER() AS m
          ALL ROWS PER MATCH
          PATTERN (x+)
          DEFINE x AS amount >= 0
        ) LIMIT 2
        """
    ).fetchall()
    assert rows, "MATCH_RECOGNIZE returned nothing"
    ok("unparseable-by-chukei SQL executes via fail-open passthrough")

    # Session parameter change must round-trip.
    cur.execute("ALTER SESSION SET TIMEZONE = 'UTC'")
    tz = cur.execute("SHOW PARAMETERS LIKE 'TIMEZONE' IN SESSION").fetchall()[0][1]
    assert tz == "UTC", tz
    ok("ALTER SESSION round-trip")


def _worker(i):
    conn = connect(session_parameters={"QUERY_TAG": f"chukei-conc-{i}"})
    cur = conn.cursor()
    cur.execute("USE SCHEMA CHUKEI_PILOT.PUBLIC")
    errors = []
    for j in range(8):
        try:
            if j % 4 == 3:
                cur.execute(f"INSERT INTO conc VALUES ({i}, {j})")
            elif j % 4 == 2:
                cur.execute("SELECT RANDOM()").fetchone()
            else:
                # identical text across workers → coalescing + cache contention
                rows = cur.execute(
                    "SELECT region, COUNT(*) FROM t GROUP BY region ORDER BY region"
                ).fetchall()
                assert len(rows) >= 3
        except Exception as e:  # collect, don't die mid-pool
            errors.append(f"worker {i} iter {j}: {e}")
    conn.close()
    return errors


def stage_concurrency(conn):
    cur = conn.cursor()
    cur.execute("USE SCHEMA CHUKEI_PILOT.PUBLIC")
    cur.execute("CREATE OR REPLACE TABLE conc (worker INT, iter INT)")
    n = 12
    with concurrent.futures.ThreadPoolExecutor(max_workers=n) as pool:
        all_errors = [e for errs in pool.map(_worker, range(n)) for e in errs]
    assert not all_errors, "\n".join(all_errors)
    inserted = cur.execute("SELECT COUNT(*) FROM conc").fetchone()[0]
    assert inserted == n * 2, f"lost writes under concurrency: {inserted}/{n * 2}"
    ok(f"{n} parallel sessions × 8 queries: no errors, no lost writes")


def stage_longrunning(conn):
    """Queries that exceed the inline-result window (~45s) switch to
    Snowflake's async result-polling flow (queryInProgress + result URL).
    Both the implicit flow and the explicit async API must transit chukei."""
    cur = conn.cursor()
    cur.execute("USE SCHEMA CHUKEI_PILOT.PUBLIC")

    t0 = time.time()
    row = cur.execute("CALL SYSTEM$WAIT(50)").fetchone()
    elapsed = time.time() - t0
    assert elapsed >= 50, f"wait returned early: {elapsed:.0f}s"
    assert "waited 50 seconds" in str(row[0]), row
    ok(f"50s sync query via async polling flow ({elapsed:.0f}s wall)")

    qid = cur.execute_async(
        "SELECT COUNT(*) FROM TABLE(GENERATOR(ROWCOUNT => 5000000)) WHERE SYSTEM$WAIT(8) IS NOT NULL"
    )["queryId"]
    while conn.is_still_running(conn.get_query_status_throw_if_error(qid)):
        time.sleep(1)
    rcur = conn.cursor()
    rcur.get_results_from_sfqid(qid)
    assert rcur.fetchone() is not None
    ok("explicit execute_async + status poll + get_results_from_sfqid")


STAGES = {
    "core": stage_core,
    "shapes": stage_shapes,
    "concurrency": stage_concurrency,
    "longrunning": stage_longrunning,
}

if __name__ == "__main__":
    names = sys.argv[1:] or ["core"]
    conn = connect()
    for name in names:
        print(f"── stage: {name} ──", flush=True)
        STAGES[name](conn)
    conn.close()
    print(f"CLIENT MATRIX: ALL PASS ({', '.join(names)})")
