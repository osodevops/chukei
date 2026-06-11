"""Overnight soak traffic: mixed workload through chukei until the stop
file appears or SOAK_HOURS elapses. Run via scripts/soak.sh.

Mix per worker iteration (roughly a BI + ETL blend):
  40% repeated dashboard query   (cache-hit path, blame-sampled)
  20% randomized-literal query   (cache-miss path)
  15% non-deterministic          (determinism gate)
  15% write + readback           (invalidation)
  10% 50k-row result             (chunk passthrough)

Long-lived connections on purpose: sessions must survive Snowflake's
~4h token rotation through chukei's session re-keying.
"""

import concurrent.futures
import os
import random
import sys
import time

sys.path.insert(0, os.path.dirname(__file__))
from live_matrix import connect  # noqa: E402

STOP = os.environ.get("SOAK_STOP_FILE", "pilot-data/soak.stop")
HOURS = float(os.environ.get("SOAK_HOURS", "8"))
WORKERS = int(os.environ.get("SOAK_WORKERS", "4"))
DEADLINE = time.time() + HOURS * 3600


def worker(i):
    random.seed(i)
    stats = {"ok": 0, "err": 0, "reconnects": 0}
    conn = connect(session_parameters={"QUERY_TAG": f"chukei-soak-{i}"})
    cur = conn.cursor()
    cur.execute("USE SCHEMA CHUKEI_PILOT.PUBLIC")
    cur.execute(f"CREATE TABLE IF NOT EXISTS soak_{i} (at INT, v INT)")
    while time.time() < DEADLINE and not os.path.exists(STOP):
        roll = random.random()
        try:
            if roll < 0.40:
                cur.execute(
                    "SELECT region, SUM(amount) FROM t GROUP BY region ORDER BY region"
                ).fetchall()
            elif roll < 0.60:
                cur.execute(
                    f"SELECT COUNT(*) FROM t WHERE amount > {random.randint(0, 50)}"
                ).fetchone()
            elif roll < 0.75:
                cur.execute("SELECT RANDOM(), CURRENT_TIMESTAMP()").fetchone()
            elif roll < 0.90:
                cur.execute(f"INSERT INTO soak_{i} VALUES ({int(time.time())}, {i})")
                cur.execute(f"SELECT COUNT(*) FROM soak_{i}").fetchone()
            else:
                c = cur.execute(
                    "SELECT seq4() FROM TABLE(GENERATOR(ROWCOUNT => 50000))"
                )
                assert sum(len(b) for b in iter(lambda: c.fetchmany(25000), [])) == 50000
            stats["ok"] += 1
        except Exception as e:
            stats["err"] += 1
            print(f"[soak-{i}] error: {type(e).__name__}: {e}", flush=True)
            try:  # one reconnect attempt, then keep looping
                conn = connect(session_parameters={"QUERY_TAG": f"chukei-soak-{i}"})
                cur = conn.cursor()
                cur.execute("USE SCHEMA CHUKEI_PILOT.PUBLIC")
                stats["reconnects"] += 1
            except Exception as e2:
                print(f"[soak-{i}] reconnect failed: {e2}", flush=True)
                time.sleep(30)
        time.sleep(random.uniform(0.5, 3.0))
    return stats


if __name__ == "__main__":
    print(f"soak: {WORKERS} workers, {HOURS}h deadline, stop file {STOP}", flush=True)
    with concurrent.futures.ThreadPoolExecutor(max_workers=WORKERS) as pool:
        results = list(pool.map(worker, range(WORKERS)))
    total_ok = sum(r["ok"] for r in results)
    total_err = sum(r["err"] for r in results)
    total_rec = sum(r["reconnects"] for r in results)
    print(f"soak done: ok={total_ok} err={total_err} reconnects={total_rec}", flush=True)
