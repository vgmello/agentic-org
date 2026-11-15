"""Executable checks for the organization-data draft, using only Python's stdlib.

Run: python3 docs/design/organization-data/test_schema.py
These exercise SQL projections; they do not implement an application importer.
"""

import hashlib
import json
from pathlib import Path
import re
import sqlite3
import unittest


ROOT = Path(__file__).resolve().parent


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def insert(connection, table, **fields):
    columns = ",".join(fields)
    placeholders = ",".join("?" for _ in fields)
    connection.execute(
        f"INSERT INTO {table} ({columns}) VALUES ({placeholders})", list(fields.values())
    )


def database():
    connection = sqlite3.connect(":memory:")
    connection.row_factory = sqlite3.Row
    connection.executescript((ROOT / "schema.sql").read_text())
    return connection


def apply_execution_extension(connection):
    connection.executescript((ROOT / "execution-schema.sql").read_text())


def seed_provider_connection(connection, identity="subscription-1", connection_id="connection-1",
                             auth_mode="subscription_cli"):
    insert(connection, "provider_connections", organization_id="org", id=connection_id,
           schema_version=1, revision=1, key=connection_id, name=connection_id,
           provider_type="example-provider", auth_mode=auth_mode,
           subscription_identity=identity, runtime_driver="example-driver")


def seed_organization(connection, organization="org"):
    common = dict(organization_id=organization, schema_version=1, revision=1)
    insert(connection, "organizations", id=organization, slug=organization,
           schema_version=1, revision=1, name=organization,
           organization_repository_id="org-repository", state_branch="main")
    insert(connection, "repositories", **common, id="org-repository", kind="remote",
           remote_url="https://example.com/organization.git")
    for team in ("payments", "platform", "consulting"):
        insert(connection, "teams", **common, id=team, name=team)
    insert(connection, "employees", **common, id="reporter", handle="reporter",
           kind="person", name="Reporter", home_team_id="payments")
    insert(connection, "employees", **common, id="ana", handle="ana",
           kind="agent", name="Ana", home_team_id="consulting")
    insert(connection, "projects", **common, id="project", slug="project",
           number=8, name="Project", owner_team_id="payments")
    connection.commit()


def seed_task(connection, task="task", project="project", task_type="bug"):
    insert(connection, "tasks", organization_id="org", id=task, schema_version=1,
           revision=1, number=42, team_id="payments", project_id=project,
           type=task_type, title="Prevent duplicate charges", status="todo",
           created_by_kind="employee", created_by_id="reporter",
           created_at="2026-11-15T10:00:00Z")
    insert(connection, "record_refs", organization_id="org", kind="task",
           id=task, is_deleted=0)
    connection.commit()


def event(revision, change, task="task", source_metadata=None):
    record = dict(
        organization_id="org", task_kind="task", task_id=task, revision=revision,
        schema_version=1, occurred_at=f"2026-11-15T10:{revision:02d}:00Z",
        actor_kind="employee", actor_id="reporter", operation_id=f"operation-{revision}",
        change=change, source_metadata=source_metadata or {},
    )
    record["payload_sha256"] = hashlib.sha256(canonical(record).encode()).hexdigest()
    return record


def project_event(connection, record):
    fields = dict(record)
    fields["change_json"] = canonical(fields.pop("change"))
    fields["source_metadata_json"] = canonical(fields.pop("source_metadata"))
    insert(connection, "task_events", **fields)


def assignment_change(team="platform", assignee="ana"):
    return {
        "type": "assignment_changed",
        "from": {"assigned_team_id": None, "assignee_id": None},
        "to": {"assigned_team_id": team, "assignee_id": assignee},
    }


class DraftSchemaTests(unittest.TestCase):
    def setUp(self):
        self.db = database()
        self.addCleanup(self.db.close)
        seed_organization(self.db)
        seed_task(self.db)

    def test_consolidated_tables_and_views_match_structural_documents(self):
        sections = []
        for path in sorted(ROOT.glob("[0-9][0-9]-*.md")):
            if not path.name.startswith("11-"):
                sections.extend(re.findall(r"```sql\n(.*?)\n```", path.read_text(), re.S))
        documented = sqlite3.connect(":memory:")
        self.addCleanup(documented.close)
        documented.executescript("\n".join(sections))

        def definitions(connection):
            return {
                (kind, name): re.sub(r"\s+", "", sql).lower()
                for kind, name, sql in connection.execute(
                    "SELECT type,name,sql FROM sqlite_schema WHERE type IN ('table','view')"
                )
            }

        self.assertEqual(definitions(documented), definitions(self.db))

    def test_execution_extension_applies_and_all_views_resolve(self):
        apply_execution_extension(self.db)
        views = self.db.execute("SELECT name FROM sqlite_schema WHERE type='view'").fetchall()
        for (name,) in views:
            self.db.execute(f'SELECT * FROM "{name}" LIMIT 0')
        self.assertEqual(self.db.execute("PRAGMA foreign_key_check").fetchall(), [])

    def test_same_subscription_cannot_be_registered_twice_or_relabelled(self):
        apply_execution_extension(self.db)
        seed_provider_connection(self.db)
        self.db.commit()
        for enabled, archived in ((1, 0), (0, 0), (0, 1)):
            self.db.execute("UPDATE provider_connections SET enabled=?,archived=?", (enabled, archived))
            self.db.commit()
            with self.subTest(enabled=enabled, archived=archived):
                with self.assertRaises(sqlite3.IntegrityError):
                    seed_provider_connection(self.db, connection_id="duplicate")
                self.db.rollback()
        for replacement in (None, "different-subscription"):
            with self.assertRaises(sqlite3.IntegrityError):
                self.db.execute("UPDATE provider_connections SET subscription_identity=?", (replacement,))
            self.db.rollback()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE provider_connections SET provider_type='another-provider'")
        self.db.rollback()
        seed_provider_connection(self.db, identity="subscription-2", connection_id="connection-2")
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT count(*) FROM provider_connections").fetchone()[0], 2)

    def test_registered_subscription_cannot_be_replaced_through_any_unique_key(self):
        apply_execution_extension(self.db)
        seed_provider_connection(self.db)
        self.db.commit()
        for connection_id, key, identity, mode in (
            ("connection-1", "new-key", "subscription-2", "subscription_cli"),
            ("connection-2", "connection-1", "subscription-2", "subscription_cli"),
            ("connection-2", "new-key", "subscription-1", "subscription_cli"),
            ("connection-1", "new-key", None, "api_key"),
        ):
            with self.subTest(connection_id=connection_id, key=key, identity=identity):
                with self.assertRaises(sqlite3.IntegrityError):
                    self.db.execute(
                        "INSERT OR REPLACE INTO provider_connections "
                        "(organization_id,id,schema_version,revision,key,name,provider_type,"
                        "auth_mode,subscription_identity,runtime_driver) "
                        "VALUES ('org',?,1,1,?,'Replacement','example-provider',?,?,'example-driver')",
                        (connection_id, key, mode, identity),
                    )
                self.db.rollback()
        row = self.db.execute("SELECT id,key,subscription_identity FROM provider_connections").fetchone()
        self.assertEqual(tuple(row), ("connection-1", "connection-1", "subscription-1"))

    def test_update_replace_cannot_displace_a_registered_subscription(self):
        apply_execution_extension(self.db)
        seed_provider_connection(self.db)
        seed_provider_connection(self.db, identity=None, connection_id="api", auth_mode="api_key")
        self.db.commit()
        for change in (
            "id='connection-1'", "key='connection-1'", "subscription_identity='subscription-1'",
        ):
            with self.subTest(change=change), self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(f"UPDATE OR REPLACE provider_connections SET {change} WHERE id='api'")
            self.db.rollback()
        row = self.db.execute(
            "SELECT subscription_identity FROM provider_connections WHERE id='connection-1'"
        ).fetchone()
        self.assertEqual(row[0], "subscription-1")
        self.assertEqual(self.db.execute("SELECT count(*) FROM provider_connections").fetchone()[0], 2)

    def test_subscription_identity_is_required_but_not_invented_for_api_connections(self):
        apply_execution_extension(self.db)
        for missing in (None, "", "   "):
            with self.subTest(identity=missing):
                with self.assertRaises(sqlite3.IntegrityError):
                    seed_provider_connection(self.db, identity=missing)
                self.db.rollback()
        seed_provider_connection(self.db, identity=None, auth_mode="api_key")
        seed_provider_connection(self.db, identity=None, connection_id="cloud", auth_mode="cloud_iam")
        self.db.commit()

    def test_two_offices_bind_one_subscription_connection(self):
        apply_execution_extension(self.db)
        seed_provider_connection(self.db)
        for office, role in (("hq", "hq"), ("east", "satellite")):
            insert(self.db, "offices", organization_id="org", id=office, slug=office,
                   schema_version=1, revision=1, name=office, role=role)
            insert(self.db, "office_connection_bindings", organization_id="org",
                   office_id=office, connection_id="connection-1", revision=1,
                   auth_context_ref=f"vault:{office}", updated_at="2026-11-15T12:00:00Z")
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT count(*) FROM office_connection_bindings").fetchone()[0], 2)
        self.assertEqual(self.db.execute("SELECT count(*) FROM provider_connections").fetchone()[0], 1)
        self.assertEqual(self.db.execute("PRAGMA foreign_key_check").fetchall(), [])

    def test_cross_team_assignment_preserves_ownership_number_and_reporter(self):
        self.db.execute(
            "UPDATE tasks SET assigned_team_id='platform',assignee_id='ana',revision=2 WHERE id='task'"
        )
        self.db.commit()
        row = self.db.execute(
            "SELECT team_id,project_id,number,created_by_id,assigned_team_id,assignee_id FROM tasks"
        ).fetchone()
        self.assertEqual(tuple(row), ("payments", "project", 42, "reporter", "platform", "ana"))
        self.db.execute("UPDATE employees SET home_team_id='payments' WHERE id='ana'")
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT assigned_team_id FROM tasks").fetchone()[0], "platform")

    def test_all_assignment_shapes_and_task_types(self):
        for task_type in ("task", "bug", "request", "incident"):
            for team, assignee in ((None, None), ("platform", None), (None, "ana"), ("platform", "ana")):
                with self.subTest(task_type=task_type, team=team, assignee=assignee):
                    self.db.execute(
                        "UPDATE tasks SET type=?,assigned_team_id=?,assignee_id=?",
                        (task_type, team, assignee),
                    )
                    self.db.commit()

    def test_projectless_task_keeps_owning_team_scope_when_assigned_elsewhere(self):
        seed_task(self.db, task="team-task", project=None, task_type="request")
        self.db.execute(
            "UPDATE tasks SET assigned_team_id='platform',assignee_id='ana' WHERE id='team-task'"
        )
        self.db.commit()
        row = self.db.execute(
            "SELECT team_id,project_id,number FROM tasks WHERE id='team-task'"
        ).fetchone()
        self.assertEqual(tuple(row), ("payments", None, 42))

    def test_cross_organization_assignment_is_rejected(self):
        seed_organization(self.db, "other-org")
        insert(self.db, "teams", organization_id="other-org", id="foreign-team",
               schema_version=1, revision=1, name="Other organization team")
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE tasks SET assigned_team_id='foreign-team'")
            self.db.commit()
        self.db.rollback()

    def test_assigning_another_team_does_not_relax_project_ownership(self):
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE tasks SET team_id='platform',assigned_team_id='consulting'")
            self.db.commit()
        self.db.rollback()

    def test_history_is_immutable_including_replace(self):
        project_event(self.db, event(1, {"type": "created"}))
        self.db.commit()
        for statement in (
            "UPDATE task_events SET actor_kind='system',actor_id=NULL",
            "DELETE FROM task_events",
            "INSERT OR REPLACE INTO task_events SELECT * FROM task_events",
        ):
            with self.subTest(statement=statement), self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(statement)
            self.db.rollback()
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM task_events").fetchone()[0], 1)

    def test_invalid_assignment_history_payloads_are_rejected(self):
        invalid_changes = [
            {}, {"type": None}, {"type": ""},
            {"type": "assignment_changed"},
            {"type": "assignment_changed", "from": {}, "to": {}},
            {"type": "assignment_changed", "from": {"assigned_team_id": None, "assignee_id": None},
             "to": {"assigned_team_id": 12, "assignee_id": "ana"}},
        ]
        for change in invalid_changes:
            with self.subTest(change=change), self.assertRaises(sqlite3.IntegrityError):
                project_event(self.db, event(1, change))
            self.db.rollback()

    def test_history_requires_task_identity_and_valid_actor(self):
        invalid = [event(1, {"type": "created"}, task="missing")]
        wrong_actor = event(1, {"type": "created"})
        wrong_actor["actor_id"] = "missing-employee"
        invalid.append(wrong_actor)
        wrong_kind = event(1, {"type": "created"})
        wrong_kind["task_kind"] = "bug"
        invalid.append(wrong_kind)
        for record in invalid:
            with self.subTest(record=record), self.assertRaises(sqlite3.IntegrityError):
                project_event(self.db, record)
                self.db.commit()
            self.db.rollback()

    def test_projection_failure_rolls_back_assignment_with_event(self):
        project_event(self.db, event(1, {"type": "created"}))
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            with self.db:
                self.db.execute("UPDATE tasks SET assigned_team_id='platform',revision=2")
                project_event(self.db, event(1, assignment_change()))
        row = self.db.execute("SELECT assigned_team_id,revision FROM tasks").fetchone()
        self.assertEqual(tuple(row), (None, 1))

    def test_history_survives_task_archival_and_fresh_projection(self):
        records = [event(1, {"type": "created"}), event(2, assignment_change())]
        for record in records:
            project_event(self.db, record)
        self.db.execute("UPDATE tasks SET assigned_team_id='platform',assignee_id='ana',revision=2")
        insert(self.db, "tombstones", organization_id="org", kind="task", id="task",
               schema_version=1, revision=2, terminal_state="done",
               record_metadata_json=canonical({"created_by_id": "reporter", "assigned_team_id": "platform", "assignee_id": "ana"}))
        self.db.execute("UPDATE record_refs SET is_deleted=1 WHERE id='task'")
        self.db.execute("DELETE FROM tasks WHERE id='task'")
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM task_events").fetchone()[0], 2)

        rebuilt = database()
        self.addCleanup(rebuilt.close)
        seed_organization(rebuilt)
        insert(rebuilt, "tombstones", **dict(self.db.execute("SELECT * FROM tombstones").fetchone()))
        insert(rebuilt, "record_refs", organization_id="org", kind="task", id="task", is_deleted=1)
        # Replay serialized archive objects, preserving hashes and revision identity.
        for record in json.loads(canonical(records)):
            unsigned = {key: value for key, value in record.items() if key != "payload_sha256"}
            self.assertEqual(record["payload_sha256"], hashlib.sha256(canonical(unsigned).encode()).hexdigest())
            project_event(rebuilt, record)
        rebuilt.commit()
        original = [dict(row) for row in self.db.execute("SELECT * FROM task_events ORDER BY revision")]
        restored = [dict(row) for row in rebuilt.execute("SELECT * FROM task_events ORDER BY revision")]
        self.assertEqual(original, restored)
        self.assertEqual(rebuilt.execute("PRAGMA foreign_key_check").fetchall(), [])

    def test_legacy_event_and_unknown_future_payload_are_retained(self):
        legacy = {
            "target_id": "task", "version": 1, "at": "2026-11-14T10:00:00Z",
            "actor_id": "reporter", "auto": False, "org_position": {"epoch": 1, "seq": 3},
            "change": {"type": "assigned", "from": None, "to": "ana"},
        }
        record = event(1, legacy["change"], source_metadata={"legacy_event": legacy})
        record["occurred_at"] = legacy["at"]
        record["payload_sha256"] = hashlib.sha256(canonical({k: v for k, v in record.items() if k != "payload_sha256"}).encode()).hexdigest()
        project_event(self.db, record)
        project_event(self.db, event(2, {"type": "future_event", "evidence": "retain me"}))
        self.db.commit()
        saved = self.db.execute("SELECT source_metadata_json FROM task_events WHERE revision=1").fetchone()[0]
        self.assertEqual(json.loads(saved)["legacy_event"], legacy)
        self.assertIsNone(self.db.execute("SELECT assigned_team_id FROM tasks").fetchone()[0])

    def test_relative_document_links_exist(self):
        for path in ROOT.glob("*.md"):
            for target in re.findall(r"\]\(([^)]+)\)", path.read_text()):
                target = target.split("#", 1)[0]
                if target and not re.match(r"[a-z]+://", target):
                    with self.subTest(document=path.name, target=target):
                        self.assertTrue((path.parent / target).exists())


class IntegrityAuditTests(unittest.TestCase):
    def setUp(self):
        self.db = database()
        self.addCleanup(self.db.close)
        seed_organization(self.db)
        seed_task(self.db)

    def delivery(self):
        common = dict(organization_id="org", schema_version=1, revision=1)
        insert(self.db, "releases", **common, id="release", project_id="project", title="Release", status="released")
        insert(self.db, "deployments", **common, id="deploy", project_id="project", release_id="release",
               environment="production", target="primary", operation="deploy", status="queued")
        self.db.commit()

    def accounting(self):
        insert(self.db, "offices", organization_id="org", id="hq", slug="hq", name="HQ", role="hq", schema_version=1, revision=1)
        insert(self.db, "model_prices", organization_id="org", id="rate", schema_version=1,
               provider="example", model="model", effective_from="2026-01-01T00:00:00Z", created_at="2026-01-01T00:00:00Z",
               created_by_kind="system", input_micro_usd_per_million=1000000)
        insert(self.db, "usage_records", organization_id="org", id="usage", schema_version=1,
               origin_office_id="hq", producer_id="producer", source_event_id="event", source_sequence=1,
               actor_kind="agent", employee_id="ana", runtime="test", provider="example", model="model",
               occurred_at="2026-11-15T12:00:00Z", recorded_at="2026-11-15T12:00:00Z", recorded_by_kind="system",
               normalization_version=1, payload_sha256="a"*64, input_tokens=1, output_tokens=0, cache_read_tokens=0,
               cache_write_tokens=0, token_status="complete", price_id="rate", cost_micro_usd=1,
               cost_basis="token_estimate", cost_status="complete", scope_kind="organization", attribution_basis="explicit_context")
        insert(self.db, "usage_stream_checkpoints", organization_id="org", origin_office_id="hq",
               producer_id="producer", high_water_sequence=1, head_sha256="a"*64, state="sealed", checkpointed_at="2026-11-15T12:00:00Z")
        self.db.commit()

    def test_execution_asset_matches_document_sql(self):
        documented = "\n\n".join(re.findall(r"```sql\n(.*?)\n```", (ROOT / "11-agent-execution-pools-addendum.md").read_text(), re.S))
        asset = (ROOT / "execution-schema.sql").read_text().split("\n", 1)[1].strip()
        self.assertEqual(asset, documented)

    def test_all_documented_diagram_fields_exist_in_their_database(self):
        apply_execution_extension(self.db)
        structural = database()
        self.addCleanup(structural.close)
        private = sqlite3.connect(":memory:")
        self.addCleanup(private.close)
        private.executescript((ROOT / "private-notes-schema.sql").read_text())
        for path in ROOT.glob("[0-9][0-9]-*.md"):
            for diagram in re.findall(r"```mermaid\n(.*?)\n```", path.read_text(), re.S):
                for table, body in re.findall(r"(\w+)\s*\{([^{}]*)\}", diagram, re.S):
                    fields = re.findall(r"^\s*(?:TEXT|INTEGER|REAL|BLOB)\s+(\w+)", body, re.M)
                    if fields:
                        db = private if table.startswith("private_") else self.db if path.name.startswith("11-") else structural
                        actual = {row[1] for row in db.execute(f'PRAGMA table_xinfo("{table}")')}
                        with self.subTest(document=path.name, table=table):
                            self.assertEqual(set(fields), actual)

    def test_snapshot_history_survives_current_repository_detachment(self):
        self.delivery()
        insert(self.db, "project_repositories", organization_id="org", project_id="project", repository_id="org-repository", integration_branch="main")
        for table, field, value in (("release_repository_commits", "release_id", "release"), ("deployment_repository_commits", "deployment_id", "deploy")):
            insert(self.db, table, organization_id="org", project_id="project", repository_id="org-repository", commit_sha="a"*40, **{field:value})
        self.db.commit()
        self.db.execute("DELETE FROM project_repositories")
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT count(*) FROM release_repository_commits").fetchone()[0], 1)
        self.assertEqual(self.db.execute("SELECT count(*) FROM deployment_repository_commits").fetchone()[0], 1)
        self.assertEqual(self.db.execute("PRAGMA foreign_key_check").fetchall(), [])

    def test_deferred_flag_definition_cannot_accept_wrong_setting_type(self):
        self.delivery()
        for table, field, value in (("release_flag_settings", "release_id", "release"), ("deployment_flag_settings", "deployment_id", "deploy")):
            with self.subTest(table=table):
                scope = dict(environment="production", target="primary") if table == "release_flag_settings" else {}
                insert(self.db, table, organization_id="org", project_id="project", flag_id="flag", value_json='"text"', **scope, **{field:value})
                with self.assertRaises(sqlite3.IntegrityError):
                    insert(self.db, "feature_flags", organization_id="org", id="flag", schema_version=1, revision=1,
                           project_id="project", key="flag", value_type="boolean", default_value_json="false")
                self.db.rollback()

    def test_valid_deferred_flags_and_used_identity_replacement(self):
        self.delivery()
        insert(self.db, "release_flag_settings", organization_id="org", release_id="release", project_id="project",
               flag_id="flag", environment="production", target="primary", value_json="false")
        insert(self.db, "deployment_flag_settings", organization_id="org", deployment_id="deploy", project_id="project", flag_id="flag", value_json="true")
        insert(self.db, "feature_flags", organization_id="org", id="flag", schema_version=1, revision=1,
               project_id="project", key="flag", value_type="boolean", default_value_json="false")
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT OR REPLACE INTO feature_flags SELECT * FROM feature_flags")
        self.db.rollback()

    def test_coverage_lists_every_table_and_view(self):
        apply_execution_extension(self.db)
        private = sqlite3.connect(":memory:")
        self.addCleanup(private.close)
        private.executescript((ROOT / "private-notes-schema.sql").read_text())
        coverage = (ROOT / "SCHEMA-COVERAGE.md").read_text()
        for db in (self.db, private):
            for (name,) in db.execute("SELECT name FROM sqlite_schema WHERE type IN ('table','view')"):
                self.assertIn(f"`{name}`", coverage)

    def test_reporting_cycles_rejected_on_update_and_deferred_insert(self):
        self.db.execute("UPDATE employees SET reports_to_employee_id='reporter' WHERE id='ana'")
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE employees SET reports_to_employee_id='ana' WHERE id='reporter'")
        self.db.rollback()
        insert(self.db, "employees", organization_id="org", id="a", handle="a", name="a", kind="person", home_team_id="payments", schema_version=1, revision=1, reports_to_employee_id="b")
        with self.assertRaises(sqlite3.IntegrityError):
            insert(self.db, "employees", organization_id="org", id="b", handle="b", name="b", kind="person", home_team_id="payments", schema_version=1, revision=1, reports_to_employee_id="a")
        self.db.rollback()

    def test_accounting_replacements_and_stream_identity_changes_are_rejected(self):
        self.accounting()
        self.assertEqual(self.db.execute("PRAGMA recursive_triggers").fetchone()[0], 1)
        for table in ("usage_records", "model_prices", "usage_stream_checkpoints"):
            with self.subTest(table=table), self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(f"INSERT OR REPLACE INTO {table} SELECT * FROM {table}")
            self.db.rollback()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE usage_stream_checkpoints SET producer_id='another'")
        self.db.rollback()
        # An unused rate cannot displace a used rate via a different unique key.
        insert(self.db, "model_prices", organization_id="org", id="other", schema_version=1, provider="example", model="model",
               effective_from="2027-01-01T00:00:00Z", created_at="2026-01-01T00:00:00Z", created_by_kind="system")
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE OR REPLACE model_prices SET effective_from='2026-01-01T00:00:00Z' WHERE id='other'")
        self.db.rollback()

    def test_historical_usage_actor_cannot_be_retyped(self):
        self.accounting()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE employees SET kind='person' WHERE id='ana'")
        self.db.rollback()


class PrivateNoteSchemaTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.row_factory = sqlite3.Row
        self.addCleanup(self.db.close)
        self.db.executescript((ROOT / "private-notes-schema.sql").read_text())
        for kind, author in (("employee", "ana"), ("employee", "reporter"), ("owner", "subject-1")):
            insert(self.db, "private_note_authors", organization_id="org", author_kind=kind, author_id=author)
        insert(self.db, "private_note_tasks", organization_id="org", task_id="task")
        insert(self.db, "private_task_notes", organization_id="org", id="note", schema_version=1, revision=1,
               task_id="task", author_kind="employee", author_id="ana", body_markdown="Private draft",
               created_at="2026-11-15T12:00:00.000Z", updated_at="2026-11-15T12:00:00.000Z")
        self.db.commit()

    def receipt(self, **overrides):
        fields=dict(organization_id="org", author_kind="employee", author_id="ana", operation_id="operation",
                    request_sha256="a"*64, command="create", note_id="note", result_revision=1, state="completed",
                    created_at="2026-11-15T12:00:00.000Z", finished_at="2026-11-15T12:00:00.000Z")
        fields.update(overrides)
        insert(self.db, "private_note_command_receipts", **fields)

    def test_private_records_are_absent_from_shared_schema(self):
        shared=database()
        self.addCleanup(shared.close)
        apply_execution_extension(shared)
        self.assertEqual(shared.execute("SELECT name FROM sqlite_schema WHERE name LIKE 'private_%'").fetchall(), [])
        self.assertEqual(self.db.execute("PRAGMA foreign_key_check").fetchall(), [])

    def test_unregistered_task_or_author_and_cross_org_notes_fail(self):
        original=dict(self.db.execute("SELECT * FROM private_task_notes").fetchone())
        for changes in ({"task_id":"missing"},{"author_id":"missing"},{"organization_id":"another"}):
            with self.subTest(changes=changes), self.assertRaises(sqlite3.IntegrityError):
                insert(self.db, "private_task_notes", **(original | {"id":"other"} | changes))
            self.db.rollback()

    def test_note_identity_author_and_revision_are_guarded(self):
        for change in ("author_id='reporter',revision=2", "task_id='missing',revision=2", "id='other',revision=2", "revision=3"):
            with self.subTest(change=change), self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(f"UPDATE private_task_notes SET {change}")
            self.db.rollback()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT OR REPLACE INTO private_task_notes SELECT * FROM private_task_notes")
        self.db.rollback()

    def test_note_survives_task_deletion_and_note_tombstone_cannot_resurrect(self):
        self.db.execute("UPDATE private_note_tasks SET is_deleted=1")
        self.db.execute("UPDATE private_task_notes SET revision=2,body_markdown=NULL,deleted_at='2026-11-15T13:00:00.000Z',updated_at='2026-11-15T13:00:00.000Z'")
        self.db.commit()
        self.assertIsNone(self.db.execute("SELECT body_markdown FROM private_task_notes").fetchone()[0])
        for statement in ("UPDATE private_task_notes SET revision=3,deleted_at=NULL,body_markdown='restored'", "DELETE FROM private_task_notes"):
            with self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(statement)
            self.db.rollback()

    def test_receipt_retains_original_result_after_later_edit(self):
        self.receipt()
        self.db.execute("UPDATE private_task_notes SET revision=2,body_markdown='Edited'")
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT result_revision FROM private_note_command_receipts").fetchone()[0], 1)
        for statement in ("INSERT OR REPLACE INTO private_note_command_receipts SELECT * FROM private_note_command_receipts", "UPDATE private_note_command_receipts SET result_revision=2"):
            with self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(statement)
            self.db.rollback()

    def test_pending_publication_locks_revision_until_outcome_is_recorded(self):
        self.receipt(command="publish", state="pending", shared_comment_id="comment-1", finished_at=None)
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE private_task_notes SET revision=2,body_markdown='Changed'")
        self.db.rollback()
        self.db.execute("UPDATE private_note_command_receipts SET state='completed',finished_at='2026-11-15T13:00:00.000Z'")
        self.db.execute("UPDATE private_task_notes SET revision=2,body_markdown='Changed'")
        self.db.commit()

    def test_receipt_cannot_impersonate_another_author(self):
        with self.assertRaises(sqlite3.IntegrityError):
            self.receipt(author_id="reporter")
        self.db.rollback()


if __name__ == "__main__":
    unittest.main(verbosity=2)
