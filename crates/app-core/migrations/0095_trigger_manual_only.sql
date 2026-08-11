-- Graph-level scheduling is gone: `Trigger` keeps only `Manual`, and a schedule is a property of a
-- `When Event` node in cron mode (one graph, several independent clocks).
--
-- THIS MIGRATION MUST LAND BEFORE the code that drops the variants. `Trigger` is an untagged-by-
-- position serde enum stored inside `workflows.data`, so a row still holding `{"Cron": {...}}` or
-- `{"Schedule": {...}}` fails to deserialize once those variants no longer exist — and the failure
-- is not "this graph lost its schedule", it is "this graph no longer loads at all". Seven graphs
-- across three tenants were in that state when this was written.
--
-- `enabled` is deliberately LEFT ALONE, unlike 0015 (which disarmed graphs when `OnNewAlert` was
-- removed). The one production graph affected — `Turn On/Off Daily` — already carries the two cron
-- nodes that replaced its schedule, and had been running on both clocks; disarming it here would
-- take a working automation down as a side effect of a cleanup. A graph left with no clock at all
-- simply never fires, which is exactly what it would do disabled.
UPDATE workflows
   SET data = jsonb_set(data, '{trigger}', '"Manual"'::jsonb)
 WHERE jsonb_typeof(data -> 'trigger') = 'object';

-- Any other shape (absent, or a string that is not "Manual") would deserialize as badly as the
-- object form. Cheap to normalise, and it makes the post-condition total rather than "the two
-- variants we happened to know about".
UPDATE workflows
   SET data = jsonb_set(data, '{trigger}', '"Manual"'::jsonb)
 WHERE data -> 'trigger' IS DISTINCT FROM '"Manual"'::jsonb;

-- The three seeded templates that fired on a graph trigger have been rebuilt around a `When Event`
-- node (motion, motion, and a daily schedule). Their rows are seeded `ON CONFLICT DO NOTHING`
-- against a stable id, so a deployment that already has them would keep the OLD bodies — which the
-- rewrite above just turned into graphs with no clock at all: they would sit in the picker looking
-- fine and teach a flow that never fires. Deleting them lets `seed_templates` re-insert the new
-- bodies at the next boot, under the same ids.
--
-- This DOES discard any visual edit an operator made to those three templates in the platform
-- tenant. Accepted deliberately: the alternative is leaving a broken lesson in every tenant's
-- picker, and the three are seeded examples, not customer graphs. A tenant's own copy (made with
-- "use template") has its own id and is untouched.
DELETE FROM workflows
 WHERE id IN (
        '5e571e10-7e37-0000-0000-000000000002',  -- Alerta de pessoa      (was Cron 2s)
        '5e571e10-7e37-0000-0000-000000000008',  -- Relatório diário      (was Schedule 08:00)
        '5e571e10-7e37-0000-0000-00000000000c'   -- Presença viva no Mapa (was Cron 3s)
       )
   AND tenant_id IN (SELECT id FROM tenants WHERE slug = 'platform');

-- `workflow_cron_state` (the graph-level poller's last-fired anchor) is now written by nothing and
-- read by nothing. It is NOT dropped here: migrations in this repo are additive, and an unused
-- table costs a row per graph that once had an interval trigger. Cron NODES keep their own state in
-- `work_jobs`, which is a different mechanism entirely.
