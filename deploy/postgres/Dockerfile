# Postgres 17 + pgvector (upstream image) + **pg_cron**.
#
# The stock `pgvector/pgvector:pg17` has no pg_cron — not even as an installable extension.
# Every `cron.schedule()` in our migrations was therefore swallowed by its `EXCEPTION`
# guard and silently never ran (the platform_logs 30-day TTL from migration 0028 among
# them). This image makes the scheduler real.
#
# Two runtime settings are required and live in the compose `command:` —
#   -c shared_preload_libraries=pg_cron   (the background worker must preload)
#   -c cron.database_name=sentinel        (pg_cron's own tables go in our DB, not `postgres`)
# Without them `CREATE EXTENSION pg_cron` fails, which is deliberate: a missing scheduler
# should break the boot, not disappear into a NOTICE.
FROM pgvector/pgvector:pg17

RUN apt-get update \
 && apt-get install -y --no-install-recommends postgresql-17-cron \
 && rm -rf /var/lib/apt/lists/*
