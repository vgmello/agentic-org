"""Executable checks for the organization-data draft, using only Python's stdlib.

Run: python3 docs/design/organization-data/test_schema.py
These exercise SQL projections; they do not implement an application importer.
"""

import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import unittest
from urllib.parse import unquote


ROOT = Path(__file__).resolve().parent
REPO_ROOT = ROOT.parents[2]
NUMBERED_DOC_RE = re.compile(r"Organization-Data-(\d{2})-")


def _resolve_backlog_root():
    env_dir = os.environ.get("ORG_BACKLOG_DIR")
    if env_dir:
        candidate = Path(env_dir).expanduser().resolve()
        if not candidate.is_dir():
            raise AssertionError(f"ORG_BACKLOG_DIR does not exist: {candidate}")
        return candidate
    candidates = [
        REPO_ROOT / ".backlog",
        REPO_ROOT.parent / "backlog",
    ]
    if len(REPO_ROOT.parents) >= 2:
        candidates.append(REPO_ROOT.parents[1] / "backlog")
    if len(REPO_ROOT.parents) >= 4:
        candidates.append(REPO_ROOT.parents[3] / "backlog")
    for candidate in candidates:
        if (candidate / "docs" / "organization-data").is_dir():
            return candidate
    return None


BACKLOG_ROOT = _resolve_backlog_root()
DOCS_ROOT = (BACKLOG_ROOT / "docs" / "organization-data") if BACKLOG_ROOT else None
DECISIONS_ROOT = (BACKLOG_ROOT / "decisions") if BACKLOG_ROOT else None
STANDALONE_SKIP_REASON = (
    "Parent backlog/ knowledge repo not present in standalone code checkout; "
    "set ORG_BACKLOG_DIR to run cross-repo doc-contract checks."
)


def _require_docs_root():
    if DOCS_ROOT is None:
        raise unittest.SkipTest(STANDALONE_SKIP_REASON)
    return DOCS_ROOT


def _require_decisions_root():
    if DECISIONS_ROOT is None:
        raise unittest.SkipTest(STANDALONE_SKIP_REASON)
    return DECISIONS_ROOT


def numbered_org_data_docs():
    docs_root = _require_docs_root()
    matched = []
    for path in docs_root.glob("doc-*Organization-Data-[0-9][0-9]-*.md"):
        match = NUMBERED_DOC_RE.search(path.name)
        if match:
            matched.append((int(match.group(1)), path))
    matched.sort(key=lambda item: item[0])
    if len(matched) != 18:
        raise AssertionError(f"Expected 18 numbered organization-data docs in {docs_root}, found {len(matched)}")
    return matched


def org_data_doc(number):
    docs_root = _require_docs_root()
    for doc_number, path in numbered_org_data_docs():
        if doc_number == number:
            return path
    raise AssertionError(f"Missing organization-data doc {number:02d} in {docs_root}")


def schema_coverage_doc():
    docs_root = _require_docs_root()
    matches = sorted(docs_root.glob("doc-74*.md"))
    if len(matches) != 1:
        raise AssertionError(f"Expected 1 doc-74 schema coverage doc in {docs_root}, found {len(matches)}")
    return matches[0]


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


# Teams, projects, releases and milestones are created only by HQ and always carry a number.
HQ_NUMBERED = {"teams", "projects", "releases", "milestones"}
_next_number = iter(range(1000, 10**9))


def insert(connection, table, **fields):
    if table in HQ_NUMBERED and "number" not in fields:
        fields["number"] = next(_next_number)
    if table == "tasks":  # a task is numbered by its creating office; the fixtures use HQ, code A
        fields.setdefault("office_code", "A")
        fields.setdefault("number", next(_next_number))
    columns = ",".join(fields)
    placeholders = ",".join("?" for _ in fields)
    connection.execute(
        f"INSERT INTO {table} ({columns}) VALUES ({placeholders})", list(fields.values())
    )


def database(schema_file="schema.sql"):
    connection = sqlite3.connect(":memory:")
    connection.row_factory = sqlite3.Row
    connection.executescript((ROOT / schema_file).read_text())
    return connection


def seed_messaging_mirrors(connection, organization="org"):
    insert(connection, "messaging_organizations", id=organization)
    for team in ("payments", "platform", "consulting"):
        insert(connection, "messaging_teams", organization_id=organization, id=team)
    insert(connection, "messaging_projects", organization_id=organization, id="project")
    for employee in ("reporter", "ana"):
        insert(connection, "messaging_employees", organization_id=organization, id=employee)
    for office in ("numbering-a", "numbering-b", "hq"):
        insert(connection, "messaging_offices", organization_id=organization, id=office)
    connection.commit()


def seed_budget_mirrors(connection, organization="org"):
    insert(connection, "budget_organizations", id=organization)
    insert(connection, "budget_tasks", organization_id=organization, kind="task", id="task", is_deleted=0)
    insert(connection, "budget_employees", organization_id=organization, id="reporter", kind="person")
    insert(connection, "budget_employees", organization_id=organization, id="ana", kind="agent")
    connection.commit()


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
    # Two offices whose only job in the fixtures is to own the task-number codes A and B. They are
    # satellites so that tests which create their own HQ office are unaffected.
    for office, code, role in (("numbering-a", "A", "satellite"), ("numbering-b", "B", "satellite")):
        insert(connection, "offices", **common, id=office, slug=office, name=office, role=role)
        insert(connection, "office_codes", organization_id=organization, code=code, office_id=office,
               issued_at="2026-11-15T09:00:00Z")
    insert(connection, "employees", **common, id="reporter", handle="reporter",
           kind="person", name="Reporter", home_team_id="payments")
    insert(connection, "employees", **common, id="ana", handle="ana",
           kind="agent", name="Ana", home_team_id="consulting")
    insert(connection, "projects", **common, id="project", slug="project",
           number=8, name="Project", owner_team_id="payments")
    connection.commit()


def seed_task(connection, task="task", project="project", task_type="bug"):
    insert(connection, "tasks", organization_id="org", id=task, schema_version=1,
           revision=1, number=42 if task == "task" else next(_next_number), team_id="payments", project_id=project,
           type=task_type, title="Prevent duplicate charges", status="todo",
           created_by_kind="employee", created_by_id="reporter",
           created_at="2026-11-15T10:00:00Z")
    insert(connection, "record_refs", organization_id="org", kind="task",
           id=task, is_deleted=0)
    connection.commit()


def event(revision, change, task="task", source_metadata=None):
    record = dict(
        organization_id="org", task_kind="task", task_id=task, revision=revision,
        schema_version=2, occurred_at=f"2026-11-15T10:{revision:02d}:00Z",
        actor_kind="employee", actor_id="reporter", operation_id=f"operation-{revision}",
        change=change, source_metadata=source_metadata or {},
    )
    record["payload_sha256"] = hashlib.sha256(canonical(record).encode()).hexdigest()
    return record


def project_event(connection, record):
    fields = dict(record)
    fields["change_json"] = canonical(fields.pop("change"))
    fields["source_metadata_json"] = canonical(fields.pop("source_metadata"))
    fields.setdefault("resource_profile", "selected-task-resources/1")
    fields.setdefault("event_bytes", canonical(record).encode())  # the event as accepted
    insert(connection, "task_events", **fields)


def assignment_change(team="platform", assignee="ana"):
    return {
        "type": "assignment_changed",
        "from": {"assigned_team_id": None, "assignee_id": None},
        "to": {"assigned_team_id": team, "assignee_id": assignee},
    }


def assert_refused(test, connection, message, statement, parameters=()):
    """Assert that a statement (SQL text or a callable) fails with this rule's own message, then
    roll back. Naming the message keeps the case tied to one trigger or index: another rule that
    happens to refuse the same write does not satisfy it."""
    with test.assertRaisesRegex(sqlite3.IntegrityError, re.escape(message)):
        statement() if callable(statement) else connection.execute(statement, parameters)
    connection.rollback()


class DraftSchemaTests(unittest.TestCase):
    def setUp(self):
        self.db = database()
        self.addCleanup(self.db.close)
        seed_organization(self.db)
        seed_task(self.db)

    def test_consolidated_tables_and_views_match_structural_documents(self):
        def definitions(connection):
            return {
                (kind, name): re.sub(r"\s+", "", sql).lower()
                for kind, name, sql in connection.execute(
                    "SELECT type,name,sql FROM sqlite_schema WHERE type IN ('table','view')"
                )
            }

        # TASK-10 (H-05): sensitive messaging and budget tables are absent from shared schema.sql.
        shared_names = {name for (_, name) in definitions(self.db)}
        for forbidden in ("conversations", "messages", "message_recipient_states", "budgets", "budget_policy_versions", "budget_holds"):
            self.assertNotIn(forbidden, shared_names)

        shared_sections = []
        isolated_docs = {
            15: "messaging-schema.sql",
            17: "budgets-schema.sql",
        }
        seen_isolated = set()
        for doc_number, path in numbered_org_data_docs():
            sql_blocks = re.findall(r"```sql\n(.*?)\n```", path.read_text(), re.S)
            if doc_number in isolated_docs:
                seen_isolated.add(doc_number)
                doc_conn = sqlite3.connect(":memory:")
                self.addCleanup(doc_conn.close)
                doc_conn.executescript("\n".join(sql_blocks))
                schema_conn = database(isolated_docs[doc_number])
                self.addCleanup(schema_conn.close)
                with self.subTest(document=path.name, schema=isolated_docs[doc_number]):
                    self.assertEqual(definitions(doc_conn), definitions(schema_conn))
            else:
                shared_sections.extend(sql_blocks)
        self.assertEqual(seen_isolated, set(isolated_docs))

        documented = sqlite3.connect(":memory:")
        self.addCleanup(documented.close)
        documented.executescript("\n".join(shared_sections))
        self.assertEqual(definitions(documented), definitions(self.db))

    def test_integrated_execution_schema_and_all_views_resolve(self):
        views = self.db.execute("SELECT name FROM sqlite_schema WHERE type='view'").fetchall()
        for (name,) in views:
            self.db.execute(f'SELECT * FROM "{name}" LIMIT 0')
        self.assertEqual(self.db.execute("PRAGMA foreign_key_check").fetchall(), [])

    def test_diagram_columns_match_their_owning_database(self):
        private = database("private-notes-schema.sql")
        self.addCleanup(private.close)
        messaging = database("messaging-schema.sql")
        self.addCleanup(messaging.close)
        budgets = database("budgets-schema.sql")
        self.addCleanup(budgets.close)
        for doc_number, path in numbered_org_data_docs():
            for diagram in re.findall(r"```mermaid\n(.*?)\n```", path.read_text(), re.S):
                for table, body in re.findall(r"^    (\w+) \{\n(.*?)^    \}", diagram, re.M | re.S):
                    columns = re.findall(r"^        \w+ (\w+)", body, re.M)
                    if table.startswith("private_"):
                        owner = private
                    elif doc_number == 15:
                        owner = messaging
                    elif doc_number == 17:
                        owner = budgets
                    else:
                        owner = self.db
                    actual = [row[1] for row in owner.execute(f"PRAGMA table_xinfo({table})")]
                    with self.subTest(document=path.name, table=table):
                        self.assertEqual(columns, actual)

    def test_placement_moves_require_a_new_generation(self):
        for identity, role in (("hq", "hq"), ("satellite", "satellite")):
            insert(self.db, "offices", organization_id="org", id=identity,
                   slug=identity, schema_version=1, revision=1, name=identity, role=role)
        insert(self.db, "agent_placements", organization_id="org", employee_id="ana", office_id="hq")
        self.db.commit()
        for statement in (
            "UPDATE agent_placements SET office_id='satellite'",
            "UPDATE agent_placements SET placement_generation=0",
        ):
            with self.subTest(statement=statement), self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(statement)
            self.db.rollback()
        self.db.execute("UPDATE agent_placements SET office_id='satellite',placement_generation=2")
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE agent_placements SET placement_generation=1")
        self.db.rollback()
        row = self.db.execute("SELECT employee_id,office_id,placement_generation FROM agent_placements").fetchone()
        self.assertEqual(tuple(row), ("ana", "satellite", 2))

    def seed_pool(self):
        seed_provider_connection(self.db)
        seed_provider_connection(self.db, identity="subscription-2", connection_id="connection-2")
        insert(self.db, "model_profiles", organization_id="org", id="pool",
               schema_version=1, revision=1, key="pool", name="Pool")
        for connection, priority in (("connection-1", 1), ("connection-2", 2)):
            insert(self.db, "model_profile_connections", organization_id="org",
                   model_profile_id="pool", connection_id=connection, priority=priority)
        self.db.commit()

    def test_profile_membership_and_agent_configuration_relationships(self):
        self.seed_pool()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE model_profile_connections SET priority=1 WHERE connection_id='connection-2'")
        self.db.rollback()
        config = dict(organization_id="org", schema_version=1, revision=1,
                      model_profile_id="pool", model_id="target-model", system_prompt_markdown="Instructions")
        with self.assertRaises(sqlite3.IntegrityError):
            insert(self.db, "agent_execution_configs", employee_id="reporter", **config)
        self.db.rollback()
        insert(self.db, "agent_execution_configs", employee_id="ana", **config)
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("DELETE FROM model_profiles WHERE id='pool'")
        self.db.rollback()
        self.assertEqual(self.db.execute("SELECT model_profile_id FROM agent_execution_configs WHERE employee_id='ana'").fetchone()[0], "pool")

    def test_agent_reasoning_effort_is_optional_and_one_of_five_levels(self):
        # TASK-181: absent (NULL) keeps the runtime's default; anything else is one of five.
        self.seed_pool()
        config = dict(organization_id="org", employee_id="ana", schema_version=1, revision=1,
                      model_profile_id="pool", model_id="target-model", system_prompt_markdown="Instructions")
        insert(self.db, "agent_execution_configs", **config)
        self.db.commit()
        self.assertIsNone(self.db.execute("SELECT effort FROM agent_execution_configs WHERE employee_id='ana'").fetchone()[0])
        for level in ("low", "medium", "high", "xhigh", "max"):
            self.db.execute("UPDATE agent_execution_configs SET effort=? WHERE employee_id='ana'", (level,))
            self.db.commit()
        for wrong in ("", "none", "XHIGH", "turbo", 3):
            with self.assertRaises(sqlite3.IntegrityError):
                self.db.execute("UPDATE agent_execution_configs SET effort=? WHERE employee_id='ana'", (wrong,))
            self.db.rollback()
        self.assertEqual(self.db.execute("SELECT effort FROM agent_execution_configs WHERE employee_id='ana'").fetchone()[0], "max")

    def test_profile_catalog_is_the_union_of_confirmed_connection_models(self):
        self.seed_pool()
        insert(self.db, "offices", organization_id="org", id="hq", slug="hq",
               schema_version=1, revision=1, name="HQ", role="hq")
        for connection in ("connection-1", "connection-2"):
            insert(self.db, "office_connection_bindings", organization_id="org", office_id="hq",
                   connection_id=connection, revision=1, auth_context_ref="vault:reference",
                   updated_at="2026-11-15T12:00:00Z")
            for model, entitlement in (("shared-model", "confirmed"), ("denied-model", "denied")):
                insert(self.db, "office_connection_models", organization_id="org", office_id="hq",
                       connection_id=connection, model_id=model, driver_model_id=model,
                       entitlement=entitlement, observed_at="2026-11-15T12:00:00Z",
                       valid_until="2026-11-15T12:05:00Z")
        rows = self.db.execute("SELECT model_id,supporting_connections,first_priority FROM model_profile_model_union").fetchall()
        self.assertEqual([tuple(row) for row in rows], [("shared-model", 2, 1)])

    def test_same_subscription_cannot_be_registered_twice_or_relabelled(self):
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
        for missing in (None, "", "   "):
            with self.subTest(identity=missing):
                with self.assertRaises(sqlite3.IntegrityError):
                    seed_provider_connection(self.db, identity=missing)
                self.db.rollback()
        seed_provider_connection(self.db, identity=None, auth_mode="api_key")
        seed_provider_connection(self.db, identity=None, connection_id="cloud", auth_mode="cloud_iam")
        self.db.commit()

    def test_two_offices_bind_one_subscription_connection(self):
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
        before = self.db.execute("SELECT key FROM tasks WHERE id='team-task'").fetchone()[0]
        self.db.execute(
            "UPDATE tasks SET assigned_team_id='platform',assignee_id='ana' WHERE id='team-task'"
        )
        self.db.commit()
        row = self.db.execute(
            "SELECT team_id,project_id,key FROM tasks WHERE id='team-task'"
        ).fetchone()
        self.assertEqual(tuple(row), ("payments", None, before))  # assignment never changes the reference

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

    def test_codec_two_projection_retains_tokens_and_requires_profile_and_source(self):
        # DDL preservation only: the codec/service independently validates the
        # hash, header, public classification and source identity before admission.
        record = event(1, {"type": "future_event"})
        record.pop("change")
        record.pop("source_metadata")
        record.update(schema_version=2,
                      change_json='{"type":"future_event","n":184467440737095516160001}',
                      source_metadata_json='{"future_field":{"n":-0.000000000000123456789E+009}}',
                      resource_profile="selected-task-resources/1",
                      event_bytes=b'{"original":"retained source bytes"}')
        invalid = dict(record)
        invalid.pop("resource_profile")
        with self.assertRaises(sqlite3.IntegrityError):
            insert(self.db, "task_events", **invalid)
        self.db.rollback()
        insert(self.db, "task_events", **record)
        self.db.commit()
        saved = dict(self.db.execute("SELECT * FROM task_events").fetchone())
        for key in ("change_json", "source_metadata_json", "event_bytes", "resource_profile"):
            self.assertEqual(saved[key], record[key])

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

    def test_unknown_future_payload_is_retained(self):
        project_event(self.db, event(1, {"type": "future_event", "evidence": "retain me"},
                                     source_metadata={"future_field": {"keep": True}}))
        self.db.commit()
        change, extra = self.db.execute("SELECT change_json, source_metadata_json FROM task_events WHERE revision=1").fetchone()
        self.assertEqual(json.loads(change)["evidence"], "retain me")
        self.assertEqual(json.loads(extra)["future_field"], {"keep": True})
        self.assertIsNone(self.db.execute("SELECT assigned_team_id FROM tasks").fetchone()[0])

    def test_every_task_event_carries_its_profile_and_bytes(self):
        record = event(1, {"type": "future_event"})
        for missing, value in (("schema_version", 1), ("resource_profile", None)):
            fields = dict(record)
            fields["change_json"] = canonical(fields.pop("change"))
            fields["source_metadata_json"] = canonical(fields.pop("source_metadata"))
            fields.update(resource_profile="selected-task-resources/1", event_bytes=b"{}")
            fields[missing] = value
            with self.subTest(field=missing), self.assertRaises(sqlite3.IntegrityError):
                insert(self.db, "task_events", **fields)
            self.db.rollback()
        # event_bytes may be omitted in SQLite projection when canonical bytes live on disk (M-09).
        fields = dict(record)
        fields["change_json"] = canonical(fields.pop("change"))
        fields["source_metadata_json"] = canonical(fields.pop("source_metadata"))
        fields.update(resource_profile="selected-task-resources/1", event_bytes=None)
        insert(self.db, "task_events", **fields)
        self.db.commit()
        self.assertIsNone(self.db.execute("SELECT event_bytes FROM task_events WHERE revision=1").fetchone()[0])

    def test_a_task_event_retry_is_refused_before_its_key_is_checked(self):
        project_event(self.db, event(1, {"type": "created"}))
        self.db.commit()
        assert_refused(self, self.db, "task events are immutable",
                       lambda: project_event(self.db, event(1, {"type": "created"})))
        project_event(self.db, event(2, {"type": "future_event"}))
        self.db.commit()

    def test_the_organization_repository_stays_remote(self):
        message = "organization repository must be remote"
        insert(self.db, "repositories", organization_id="org", id="spare", schema_version=1, revision=1,
               kind="local")
        self.db.commit()  # a local repository that is not the organization's is allowed
        assert_refused(self, self.db, message,
                       "UPDATE repositories SET kind='local',remote_url=NULL WHERE id='org-repository'")
        assert_refused(self, self.db, message, "UPDATE organizations SET organization_repository_id='spare'")
        self.db.execute("UPDATE repositories SET kind='remote',remote_url='https://example.com/spare.git' WHERE id='spare'")
        self.db.execute("UPDATE organizations SET organization_repository_id='spare'")
        self.db.commit()
        # A new organization, written before its repository or (with every key deferred) after it.
        new = dict(id="new", slug="new", schema_version=1, revision=1, name="New", state_branch="main")
        repository = dict(organization_id="new", schema_version=1, revision=1)
        insert(self.db, "organizations", **new, organization_repository_id="later")
        assert_refused(self, self.db, message,
                       lambda: insert(self.db, "repositories", **repository, id="later", kind="local"))
        self.db.execute("PRAGMA defer_foreign_keys=ON")
        insert(self.db, "repositories", **repository, id="early", kind="local")
        assert_refused(self, self.db, message,
                       lambda: insert(self.db, "organizations", **new, organization_repository_id="early"))
        insert(self.db, "organizations", **new, organization_repository_id="later")
        insert(self.db, "repositories", **repository, id="later", kind="remote",
               remote_url="https://example.com/new.git")
        self.db.commit()

    def test_team_tree_has_no_cycle_on_insert_or_update(self):
        self.db.execute("UPDATE teams SET parent_team_id='platform' WHERE id='payments'")
        insert(self.db, "teams", organization_id="org", id="child", schema_version=1, revision=1,
               name="Child", parent_team_id="payments")
        self.db.commit()
        assert_refused(self, self.db, "team cycle", "UPDATE teams SET parent_team_id='child' WHERE id='platform'")
        insert(self.db, "teams", organization_id="org", id="x", schema_version=1, revision=1, name="X",
               parent_team_id="y")  # the parent is written later in the same transaction
        assert_refused(self, self.db, "team cycle", lambda: insert(
            self.db, "teams", organization_id="org", id="y", schema_version=1, revision=1, name="Y",
            parent_team_id="x"))

    def test_task_dependencies_have_no_cycle_on_insert_or_update(self):
        seed_task(self.db, task="b")
        seed_task(self.db, task="c")

        def link(identity, source, target, kind="depends_on"):
            insert(self.db, "links", organization_id="org", source_kind="task", source_id=source, id=identity,
                   kind=kind, target_kind="task", target_id=target)

        link("1", "task", "b")
        link("2", "b", "c")
        self.db.commit()
        assert_refused(self, self.db, "task dependency cycle", lambda: link("3", "c", "task"))
        link("3", "c", "task", kind="relates_to")
        self.db.commit()
        assert_refused(self, self.db, "task dependency cycle", "UPDATE links SET kind='depends_on' WHERE id='3'")
        self.db.execute("UPDATE links SET target_id='c' WHERE id='1'")  # task -> c, b -> c: no cycle
        self.db.commit()

    def seed_execution(self):
        self.seed_pool()
        for office, role in (("hq", "hq"), ("east", "satellite")):
            insert(self.db, "offices", organization_id="org", id=office, slug=office, schema_version=1,
                   revision=1, name=office, role=role)
        insert(self.db, "agent_execution_configs", organization_id="org", employee_id="ana", schema_version=1,
               revision=1, model_profile_id="pool", model_id="target-model", system_prompt_markdown="Instructions")
        insert(self.db, "agent_placements", organization_id="org", employee_id="ana", office_id="hq")
        self.db.commit()

    def process(self, identity, state="planned", office="hq"):
        insert(self.db, "agent_process_instances", organization_id="org", id=identity, employee_id="ana",
               office_id=office, placement_generation=1, connection_id="connection-1",
               runtime_driver="example-driver", driver_version="1", model_id="target-model",
               resolved_model_id="target-model", connection_revision=1, workspace_key="workspace",
               launch_digest="a" * 64, binding_revision=1, session_mark=identity, state=state,
               updated_at="2026-11-15T12:00:00Z")

    def turn(self, identity, status="running", **fields):
        insert(self.db, "agent_execution_turns", **(dict(
            organization_id="org", id=identity, employee_id="ana", office_id="hq", placement_generation=1,
            revision=1, workspace_key="workspace", input_ref="input", input_sha256="a" * 64,
            agent_config_revision=1, model_profile_id="pool", profile_revision=1, model_id="target-model",
            pool_snapshot_json="[]", status=status, created_at="2026-11-15T12:00:00Z") | fields))

    def test_an_agent_has_one_open_turn_and_one_live_main_process(self):
        self.seed_execution()
        self.turn("open")
        self.turn("finished", "succeeded")
        self.turn("resumed", "failed", started_by_kind="resume")
        self.turn("bg-wake", "succeeded", started_by_kind="background_wake")
        self.turn("rev-req", "succeeded", started_by_kind="review_requested", started_by_employee_id="reporter")
        self.turn("rev-verdict", "succeeded", started_by_kind="review_verdict", started_by_employee_id="ana")
        self.turn("rev-reply", "succeeded", started_by_kind="review_reply", started_by_employee_id="reporter")
        self.process("old", "gone")
        self.process("live", "starting")
        self.db.commit()
        assert_refused(self, self.db, "UNIQUE constraint failed: agent_execution_turns",
                       lambda: self.turn("second", "blocked"))
        assert_refused(self, self.db, "CHECK constraint failed",
                       lambda: self.turn("unknown-start", "succeeded", started_by_kind="restart"))
        assert_refused(self, self.db, "CHECK constraint failed",
                       lambda: self.turn("bg-wake-emp", "succeeded", started_by_kind="background_wake", started_by_employee_id="ana"))
        assert_refused(self, self.db, "UNIQUE constraint failed: agent_process_instances",
                       lambda: self.process("another"))

    def test_execution_config_and_placement_belong_to_an_agent(self):
        self.seed_execution()
        assert_refused(self, self.db, "execution config requires an agent employee",
                       "UPDATE agent_execution_configs SET employee_id='reporter'")
        assert_refused(self, self.db, "an employee's kind never changes",
                       "UPDATE employees SET kind='person' WHERE id='ana'")
        assert_refused(self, self.db, "only an agent is placed in an office", lambda: insert(
            self.db, "agent_placements", organization_id="org", employee_id="reporter", office_id="hq"))
        assert_refused(self, self.db, "only an agent is placed in an office",
                       "UPDATE agent_placements SET employee_id='reporter'")

    def test_a_placed_employee_stays_an_agent_and_a_team_has_a_member_limit(self):
        self.seed_execution()
        insert(self.db, "employees", organization_id="org", id="placed", handle="placed", kind="agent",
               name="Placed", home_team_id="payments", schema_version=1, revision=1)
        insert(self.db, "agent_placements", organization_id="org", employee_id="placed", office_id="hq")
        self.db.commit()
        assert_refused(self, self.db, "an employee's kind never changes",
                       "UPDATE employees SET kind='person' WHERE id='placed'")
        assert_refused(self, self.db, "an employee's kind never changes",
                       "UPDATE employees SET kind='agent' WHERE id='reporter'")
        self.db.execute("UPDATE employees SET name='Placed again', kind='agent' WHERE id='placed'")
        self.assertEqual(self.db.execute("SELECT max_members FROM teams WHERE id='payments'").fetchone()[0], 6)
        self.db.execute("UPDATE teams SET max_members=12 WHERE id='payments'")
        assert_refused(self, self.db, "CHECK constraint failed",
                       "UPDATE teams SET max_members=0 WHERE id='payments'")
        insert(self.db, "employees", organization_id="org", id="bo", handle="bo", schema_version=1,
               revision=1, kind="agent", name="Bo", home_team_id="payments")
        self.db.execute("UPDATE agent_execution_configs SET employee_id='bo'")
        insert(self.db, "agent_placements", organization_id="org", employee_id="bo", office_id="east")
        insert(self.db, "employees", organization_id="org", id="cy", handle="cy", schema_version=1,
               revision=1, kind="agent", name="Cy", home_team_id="payments")
        self.db.commit()
        assert_refused(self, self.db, "an employee's kind never changes",
                       "UPDATE employees SET kind='person' WHERE id='cy'")  # unconfigured, unplaced

    def test_a_cleanup_record_is_in_its_process_office(self):
        self.seed_execution()
        self.process("process")
        self.db.commit()
        message = "cleanup record and its process are in different offices"
        record = dict(organization_id="org", id="cleanup", office_id="hq", execution_process_id="process",
                      generation=1, ownership_mark="mark", machine_id="machine", inventory_json="{}",
                      state="owned", updated_at="2026-11-15T12:00:00Z")
        assert_refused(self, self.db, message,
                       lambda: insert(self.db, "process_cleanup_records", **(record | {"office_id": "east"})))
        insert(self.db, "process_cleanup_records", **record)
        insert(self.db, "process_cleanup_records", **(record | dict(id="other", office_id="east",
                                                                    execution_process_id=None)))
        self.db.commit()
        assert_refused(self, self.db, message, "UPDATE process_cleanup_records SET office_id='east' WHERE id='cleanup'")
        assert_refused(self, self.db, message,
                       "UPDATE process_cleanup_records SET execution_process_id='process' WHERE id='other'")

    def test_relative_document_links_exist(self):
        docs = sorted(_require_docs_root().glob("*.md"))
        self.assertGreaterEqual(len(docs), 21)
        for path in docs:
            in_fence = False
            for line in path.read_text().splitlines():
                if line.lstrip().startswith(("```", "~~~")):
                    in_fence = not in_fence
                    continue
                if in_fence:
                    continue
                for target in re.findall(r"\]\(([^)]+)\)", line):
                    target = target.strip()
                    if target.startswith("<") and ">" in target:
                        target = target[1:target.index(">")]
                    else:
                        target = target.split(' "', 1)[0].strip()
                    target = unquote(target.split("#", 1)[0])
                    if target and not re.match(r"[a-z]+://", target) and not target.startswith("mailto:"):
                        with self.subTest(document=path.name, target=target):
                            in_backlog = (path.parent / target).exists()
                            in_code_repo = target.startswith("../../../") and (
                                REPO_ROOT / target.removeprefix("../../../")
                            ).exists()
                            self.assertTrue(in_backlog or in_code_repo)


class IntegrityAuditTests(unittest.TestCase):
    def setUp(self):
        self.db = database()
        self.addCleanup(self.db.close)
        seed_organization(self.db)
        seed_task(self.db)

    def delivery(self):
        common = dict(organization_id="org", schema_version=1, revision=1)
        insert(self.db, "releases", **common, id="release", project_id="project", title="Release")
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

    def test_execution_definitions_are_in_the_consolidated_schema(self):
        documented = "\n\n".join(re.findall(r"```sql\n(.*?)\n```", org_data_doc(11).read_text(), re.S))
        normalize = lambda value: re.sub(r"\s+", "", value).lower()
        self.assertIn(normalize(documented), normalize((ROOT / "schema.sql").read_text()))

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
        private = database("private-notes-schema.sql")
        self.addCleanup(private.close)
        messaging = database("messaging-schema.sql")
        self.addCleanup(messaging.close)
        budgets = database("budgets-schema.sql")
        self.addCleanup(budgets.close)
        coverage = schema_coverage_doc().read_text()
        for db in (self.db, private, messaging, budgets):
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

    def flag(self, identity="flag"):
        insert(self.db, "feature_flags", organization_id="org", id=identity, schema_version=1, revision=1,
               project_id="project", key=identity, value_type="boolean", default_value_json="false")

    def test_flag_settings_must_match_an_existing_flag_type_on_insert_and_update(self):
        self.delivery()
        self.flag()
        self.db.commit()
        for table, scope in (("release_flag_settings", dict(release_id="release", environment="production",
                                                             target="primary")),
                             ("deployment_flag_settings", dict(deployment_id="deploy"))):
            with self.subTest(table=table):
                setting = dict(organization_id="org", project_id="project", flag_id="flag", **scope)
                assert_refused(self, self.db, "flag value type mismatch",
                               lambda: insert(self.db, table, **setting, value_json='"on"'))
                insert(self.db, table, **setting, value_json="true")
                self.db.commit()
                assert_refused(self, self.db, "flag value type mismatch", f"UPDATE {table} SET value_json='\"on\"'")
                self.db.execute(f"UPDATE {table} SET value_json='false'")
                self.db.commit()

    def test_a_used_flag_keeps_its_identity(self):
        self.delivery()
        self.flag()
        self.flag("unused")
        insert(self.db, "deployment_flag_settings", organization_id="org", deployment_id="deploy",
               project_id="project", flag_id="flag", value_json="true")
        self.db.commit()
        self.db.execute("UPDATE feature_flags SET key='renamed' WHERE id='unused'")
        self.db.commit()
        assert_refused(self, self.db, "used flag identity is immutable",
                       "UPDATE feature_flags SET key='renamed-again' WHERE id='flag'")

    def usage(self, identity, sequence, **overrides):
        """Another record from the accounting fixture's producer, copied from its first record."""
        record = dict(self.db.execute("SELECT * FROM usage_records WHERE id='usage'").fetchone())
        record.update(id=identity, source_event_id=f"event-{identity}", source_sequence=sequence, **overrides)
        insert(self.db, "usage_records", **record)

    def test_usage_records_and_used_prices_are_immutable(self):
        self.accounting()
        assert_refused(self, self.db, "usage records are immutable", "UPDATE usage_records SET runtime='other'")
        assert_refused(self, self.db, "used price versions are immutable",
                       "UPDATE model_prices SET input_micro_usd_per_million=2 WHERE id='rate'")
        insert(self.db, "model_prices", organization_id="org", id="later", schema_version=1, provider="example",
               model="model", effective_from="2027-01-01T00:00:00Z", created_at="2026-01-01T00:00:00Z",
               created_by_kind="system")
        self.db.execute("UPDATE model_prices SET input_micro_usd_per_million=2 WHERE id='later'")  # unused
        self.db.commit()

    def test_agent_usage_belongs_to_an_agent_even_one_written_first(self):
        self.accounting()
        assert_refused(self, self.db, "usage actor must be an agent",
                       lambda: self.usage("person", 2, employee_id="reporter"))
        self.usage("second", 2)
        self.db.commit()
        employee = dict(organization_id="org", id="newcomer", handle="newcomer", schema_version=1, revision=1,
                        name="Newcomer", home_team_id="payments")
        self.usage("early", 3, employee_id="newcomer")  # the employee row arrives later
        assert_refused(self, self.db, "historical usage consumer must remain an agent",
                       lambda: insert(self.db, "employees", **employee, kind="person"))
        self.usage("early", 3, employee_id="newcomer")
        insert(self.db, "employees", **employee, kind="agent")
        self.db.commit()

    def test_a_correction_needs_its_parent_and_an_attribution_correction_keeps_the_facts(self):
        self.accounting()
        correction = dict(correction_kind="attribution", correction_reason="Wrong team")
        assert_refused(self, self.db, "correction parent must already exist",
                       lambda: self.usage("orphan", 2, supersedes_id="missing", **correction))
        assert_refused(self, self.db, "attribution correction cannot change consumption or valuation",
                       lambda: self.usage("moved", 2, supersedes_id="usage", input_tokens=2, **correction))
        self.usage("moved", 2, supersedes_id="usage", scope_kind="team", team_id="platform",
                   attribution_basis="manual_correction", **correction)
        self.db.commit()


class PrivateNoteSchemaTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.row_factory = sqlite3.Row
        self.addCleanup(self.db.close)
        self.db.executescript((ROOT / "private-notes-schema.sql").read_text())
        for author in ("ana", "reporter"):
            insert(self.db, "private_note_authors", organization_id="org", author_id=author)
        insert(self.db, "private_note_tasks", organization_id="org", task_id="task")
        insert(self.db, "private_notes", organization_id="org", id="note", schema_version=1, revision=1,
               task_id="task", author_id="ana", body_markdown="Private draft",
               created_at="2026-11-15T12:00:00.000Z", updated_at="2026-11-15T12:00:00.000Z")
        self.db.commit()

    def receipt(self, **overrides):
        fields=dict(organization_id="org", author_id="ana", operation_id="operation",
                    request_sha256="a"*64, command="create", note_id="note", result_revision=1, state="completed",
                    created_at="2026-11-15T12:00:00.000Z", finished_at="2026-11-15T12:00:00.000Z")
        fields.update(overrides)
        insert(self.db, "private_note_command_receipts", **fields)

    def test_private_records_are_absent_from_shared_schema(self):
        shared=database()
        self.addCleanup(shared.close)
        self.assertEqual(shared.execute("SELECT name FROM sqlite_schema WHERE name LIKE 'private_%'").fetchall(), [])
        self.assertEqual(self.db.execute("PRAGMA foreign_key_check").fetchall(), [])

    def test_unregistered_task_or_author_and_cross_org_notes_fail(self):
        original=dict(self.db.execute("SELECT * FROM private_notes").fetchone())
        for changes in ({"task_id":"missing"},{"author_id":"missing"},{"organization_id":"another"}):
            with self.subTest(changes=changes), self.assertRaises(sqlite3.IntegrityError):
                insert(self.db, "private_notes", **(original | {"id":"other"} | changes))
            self.db.rollback()

    def test_note_identity_author_and_revision_are_guarded(self):
        for change in ("author_id='reporter',revision=2", "task_id='missing',revision=2", "id='other',revision=2", "revision=3"):
            with self.subTest(change=change), self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(f"UPDATE private_notes SET {change}")
            self.db.rollback()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT OR REPLACE INTO private_notes SELECT * FROM private_notes")
        self.db.rollback()

    def test_note_survives_task_deletion_and_note_tombstone_cannot_resurrect(self):
        self.db.execute("UPDATE private_note_tasks SET is_deleted=1")
        self.db.execute("UPDATE private_notes SET revision=2,body_markdown=NULL,deleted_at='2026-11-15T13:00:00.000Z',updated_at='2026-11-15T13:00:00.000Z'")
        self.db.commit()
        self.assertIsNone(self.db.execute("SELECT body_markdown FROM private_notes").fetchone()[0])
        for statement in ("UPDATE private_notes SET revision=3,deleted_at=NULL,body_markdown='restored'", "DELETE FROM private_notes"):
            with self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(statement)
            self.db.rollback()

    def test_receipt_retains_original_result_after_later_edit(self):
        self.receipt()
        self.db.execute("UPDATE private_notes SET revision=2,body_markdown='Edited'")
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
            self.db.execute("UPDATE private_notes SET revision=2,body_markdown='Changed'")
        self.db.rollback()
        self.db.execute("UPDATE private_note_command_receipts SET state='completed',finished_at='2026-11-15T13:00:00.000Z'")
        self.db.execute("UPDATE private_notes SET revision=2,body_markdown='Changed'")
        self.db.commit()

    def test_receipt_cannot_impersonate_another_author(self):
        with self.assertRaises(sqlite3.IntegrityError):
            self.receipt(author_id="reporter")
        self.db.rollback()

    def note(self, note_id, **overrides):
        fields = dict(organization_id="org", id=note_id, schema_version=1, revision=1, task_id=None,
                      author_id="ana", kind="decision", title="Use WAL", body_markdown="Because",
                      created_at="2026-11-15T12:00:00.000Z", updated_at="2026-11-15T12:00:00.000Z")
        fields.update(overrides)
        insert(self.db, "private_notes", **fields)

    def test_a_note_may_stand_outside_any_task_but_a_named_task_must_be_mirrored(self):
        self.note("free", project_id="project")
        self.db.commit()
        self.assertEqual(self.db.execute("PRAGMA foreign_key_check").fetchall(), [])
        with self.assertRaises(sqlite3.IntegrityError):
            self.note("orphan", task_id="missing")
        self.db.rollback()
        for change in ("task_id='task'", "project_id='other'", "project_id=NULL"):
            assert_refused(self, self.db, "private note identity, revision or deletion conflict",
                           f"UPDATE private_notes SET {change},revision=2 WHERE id='free'")

    def test_kind_is_note_plan_or_decision_and_the_author_may_change_it(self):
        self.assertEqual(self.db.execute("SELECT kind FROM private_notes WHERE id='note'").fetchone()[0], "note")
        with self.assertRaises(sqlite3.IntegrityError):
            self.note("memo", kind="memo")
        self.db.rollback()
        self.db.execute("UPDATE private_notes SET kind='plan',revision=2 WHERE id='note'")
        self.db.execute("UPDATE private_notes SET kind='decision',title='Decided',revision=3 WHERE id='note'")
        self.db.commit()

    def test_an_active_decision_has_a_title_and_text_fields_are_bounded(self):
        for overrides in ({"title": None}, {"title": "   "}, {"title": "t" * 201},
                          {"why": "w" * 2001}, {"alternatives": "a" * 2001}, {"why": " "}):
            with self.subTest(overrides=overrides), self.assertRaises(sqlite3.IntegrityError):
                self.note("bounded", **overrides)
            self.db.rollback()
        self.note("bounded", title="é" * 200, why="é" * 2000, alternatives="a" * 2000)
        self.note("plain-note", kind="note", title=None)
        self.db.commit()

    def test_tags_follow_the_label_grammar_cap_at_eight_and_need_an_active_note(self):
        for tag in ("Upper", "-lead", "trail-", "two words", "x" * 65, ""):
            with self.subTest(tag=tag), self.assertRaises(sqlite3.IntegrityError):
                insert(self.db, "private_note_tags", organization_id="org", note_id="note", tag=tag)
            self.db.rollback()
        with self.assertRaises(sqlite3.IntegrityError):
            insert(self.db, "private_note_tags", organization_id="org", note_id="missing", tag="arch")
        self.db.rollback()
        for n in range(8):
            insert(self.db, "private_note_tags", organization_id="org", note_id="note", tag=f"t{n}.a:b/c_d")
        self.db.commit()
        assert_refused(self, self.db, "a private note carries at most 8 tags", lambda: insert(
            self.db, "private_note_tags", organization_id="org", note_id="note", tag="ninth"))
        assert_refused(self, self.db, "private note tags are replaced, never updated", lambda: self.db.execute(
            "UPDATE private_note_tags SET tag='renamed' WHERE note_id='note' AND tag='t0.a:b/c_d'"))

    def test_a_tombstone_clears_every_private_field_and_tag(self):
        self.note("decided", why="Private why", alternatives="Private options")
        insert(self.db, "private_note_tags", organization_id="org", note_id="decided", tag="arch")
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):  # a tombstone may not keep its title
            self.db.execute("UPDATE private_notes SET revision=2,body_markdown=NULL,why=NULL,alternatives=NULL,"
                            "deleted_at='2026-11-15T13:00:00.000Z',updated_at='2026-11-15T13:00:00.000Z' "
                            "WHERE id='decided'")
        self.db.rollback()
        self.db.execute("UPDATE private_notes SET revision=2,title=NULL,why=NULL,alternatives=NULL,body_markdown=NULL,"
                        "deleted_at='2026-11-15T13:00:00.000Z',updated_at='2026-11-15T13:00:00.000Z' WHERE id='decided'")
        self.db.commit()
        row = self.db.execute("SELECT kind,title,why,alternatives,body_markdown FROM private_notes "
                              "WHERE id='decided'").fetchone()
        self.assertEqual(tuple(row), ("decision", None, None, None, None))
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM private_note_tags").fetchone()[0], 0)
        assert_refused(self, self.db, "tags belong only to an active private note", lambda: insert(
            self.db, "private_note_tags", organization_id="org", note_id="decided", tag="late"))

    def test_only_a_task_note_can_be_published(self):
        self.note("free")
        self.db.commit()
        assert_refused(self, self.db, "private receipt must match note author and accepted revision",
                       lambda: self.receipt(operation_id="publish-free", command="publish", note_id="free",
                                            shared_comment_id="comment-free"))
        self.receipt(operation_id="publish-task", command="publish", shared_comment_id="comment-task")
        self.db.commit()

    def test_recall_queries_use_the_author_kind_and_tag_indexes(self):
        def plan_lines(conn, sql, params=()):
            return [row[3] for row in conn.execute(f"EXPLAIN QUERY PLAN {sql}", params).fetchall()]

        by_kind = "\n".join(plan_lines(
            self.db,
            "SELECT id, title FROM private_notes WHERE organization_id=? AND author_id=? AND kind=? "
            "AND deleted_at IS NULL ORDER BY updated_at DESC",
            ("org", "ana", "decision"),
        ))
        self.assertIn("private_notes_by_author_kind", by_kind)
        by_tag = "\n".join(plan_lines(
            self.db, "SELECT note_id FROM private_note_tags WHERE organization_id=? AND tag=?", ("org", "arch")))
        self.assertIn("private_note_tags_by_tag", by_tag)

    def test_a_private_note_author_is_an_employee_id_with_no_kind(self):
        for table in ("private_note_authors", "private_notes", "private_note_command_receipts"):
            with self.subTest(table=table):
                columns = [row[1] for row in self.db.execute(f"PRAGMA table_xinfo({table})")]
                self.assertIn("author_id", columns)
                self.assertNotIn("author_kind", columns)


ACTOR_COLUMN = re.compile(r"^\s*(\w*_by_kind|actor_kind)\s+TEXT\b.*?\bIN\s*\(([^)]*)\)", re.M)


class ActorAndAddresseeTests(unittest.TestCase):
    """Who did something is an employee (a person or an agent) or the system, never 'owner'.
    A column that addresses someone may still name the organization owner."""

    def setUp(self):
        self.db = database()
        self.addCleanup(self.db.close)
        seed_organization(self.db)
        seed_task(self.db)

    def test_no_actor_column_accepts_owner(self):
        found = []
        budgets_db = database("budgets-schema.sql")
        self.addCleanup(budgets_db.close)
        messaging_db = database("messaging-schema.sql")
        self.addCleanup(messaging_db.close)
        for conn in (self.db, budgets_db, messaging_db):
            for table, sql in conn.execute("SELECT name, sql FROM sqlite_schema WHERE type='table'"):
                for column, values in ACTOR_COLUMN.findall(sql):
                    found.append((table, column))
                    with self.subTest(table=table, column=column):
                        self.assertNotIn("'owner'", values)
        self.assertGreaterEqual(len(found), 30)
        self.assertNotIn("changed_by_kind",
                         [row[1] for row in budgets_db.execute("PRAGMA table_xinfo(budget_policy_versions)")])
        self.assertNotIn("author_kind", [row[1] for row in self.db.execute("PRAGMA table_xinfo(comments)")])

    def test_an_owner_actor_is_refused(self):
        statements = (
            ("UPDATE tasks SET updated_by_kind='owner', updated_by_id=NULL", ()),
            ("UPDATE employees SET created_by_kind='owner' WHERE id='ana'", ()),
            ("INSERT INTO mutation_receipts (organization_id,command_id,origin_office_id,actor_kind,actor_id,command_kind,"
             "request_sha256,state,created_at,updated_at) VALUES ('org','c','numbering-a','owner',NULL,'task.create',?,"
             "'reserved','2026-11-15T00:00:00Z','2026-11-15T00:00:00Z')", ("a" * 64,)),
        )
        for statement, parameters in statements:
            with self.subTest(statement=statement), self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(statement, parameters)
            self.db.rollback()
        record = event(2, {"type": "status_changed"})
        record.update(actor_kind="owner", actor_id=None)
        with self.assertRaises(sqlite3.IntegrityError):
            project_event(self.db, record)
        self.db.rollback()

    def test_the_owner_as_a_person_is_an_ordinary_employee_actor(self):
        self.db.execute("UPDATE tasks SET updated_by_kind='employee', updated_by_id='reporter'")
        self.db.commit()
        row = self.db.execute("SELECT updated_by_kind, updated_by_id FROM tasks").fetchone()
        self.assertEqual(tuple(row), ("employee", "reporter"))

    def comment(self, **overrides):
        insert(self.db, "comments", **(dict(organization_id="org", target_kind="task", target_id="task", id="comment",
               position=0, author_id="reporter", created_at="2026-11-15T10:00:00Z", body_markdown="Question") | overrides))

    def test_a_comment_author_is_an_existing_employee(self):
        self.comment()
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            self.comment(id="no-author", position=1, author_id=None)
        self.db.rollback()
        self.comment(id="unknown-author", position=1, author_id="nobody")
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.commit()
        self.db.rollback()

    def test_the_owner_can_still_be_addressed(self):
        self.comment()
        insert(self.db, "watchers", organization_id="org", target_kind="task", target_id="task", id="w-owner",
               watcher_kind="owner", watcher_id=None)
        insert(self.db, "watchers", organization_id="org", target_kind="task", target_id="task", id="w-ana",
               watcher_kind="employee", watcher_id="ana")
        insert(self.db, "owner_questions", organization_id="org", target_kind="task", target_id="task", id="q",
               question_comment_id="comment", asked_by_id="ana", state="open", created_at="2026-11-15T10:00:00Z")
        insert(self.db, "waiting_spans", organization_id="org", target_kind="task", target_id="task", id="span",
               reason="Owner decision needed", waiting_on_kind="owner", waiting_on_id=None, question_id="q",
               started_at="2026-11-15T10:00:00Z")
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            insert(self.db, "watchers", organization_id="org", target_kind="task", target_id="task", id="w-owner-2",
                   watcher_kind="owner", watcher_id=None)
        self.db.rollback()
        with self.assertRaises(sqlite3.IntegrityError):
            insert(self.db, "watchers", organization_id="org", target_kind="task", target_id="task", id="w-bad",
                   watcher_kind="owner", watcher_id="reporter")
        self.db.rollback()

    def test_an_employee_watches_a_task_once(self):
        watcher = dict(organization_id="org", target_kind="task", target_id="task", watcher_kind="employee")
        insert(self.db, "watchers", **watcher, id="w-ana", watcher_id="ana")
        insert(self.db, "watchers", **watcher, id="w-reporter", watcher_id="reporter")
        self.db.commit()
        assert_refused(self, self.db, "UNIQUE constraint failed: watchers",
                       lambda: insert(self.db, "watchers", **watcher, id="w-ana-2", watcher_id="ana"))


class GapPreservationTests(unittest.TestCase):
    def setUp(self):
        self.db = database()
        self.addCleanup(self.db.close)
        seed_organization(self.db)
        seed_task(self.db)
        insert(self.db, "offices", organization_id="org", id="hq", slug="hq", schema_version=1, revision=1, name="HQ", role="hq")
        self.db.commit()
        self.msg_db = database("messaging-schema.sql")
        self.addCleanup(self.msg_db.close)
        seed_messaging_mirrors(self.msg_db)
        for conversation in ("dm", "other"):
            insert(self.msg_db, "conversations", organization_id="org", id=conversation, kind="dm", created_at="2026-11-15T12:00:00Z")
            for employee in ("ana", "reporter"):
                insert(self.msg_db, "conversation_memberships", organization_id="org", conversation_id=conversation,
                       employee_id=employee, joined_at="2026-11-15T12:00:00Z")
        self.msg_db.commit()

    def message(self, identity="message", conversation="dm", sequence=1, **overrides):
        fields=dict(organization_id="org", id=identity, conversation_id=conversation, sender_kind="employee",
                    sender_employee_id="ana", sender_display_name="Ana", kind="request", body_markdown="Review?",
                    created_at="2026-11-15T12:00:00Z", origin_office_id="hq", origin_incarnation="incarnation",
                    origin_sequence=sequence, org_epoch=1, org_sequence=1, content_sha256="a"*64)
        insert(self.msg_db,"messages",**(fields | overrides))

    def test_reply_cannot_cross_conversation_and_origin_cannot_duplicate(self):
        self.message(); self.msg_db.commit()
        self.message("cross", "other", 2, reply_to_message_id="message")
        with self.assertRaises(sqlite3.IntegrityError): self.msg_db.commit()
        self.msg_db.rollback()
        with self.assertRaises(sqlite3.IntegrityError): self.message("duplicate")
        self.msg_db.rollback()
        self.message("reply", sequence=2, reply_to_message_id="message")
        self.msg_db.commit()

    def test_read_markers_are_conversation_scoped_and_mutes_are_independent(self):
        self.message(); self.msg_db.commit()
        insert(self.msg_db,"conversation_read_markers", organization_id="org",conversation_id="other",viewer_employee_id="reporter",last_read_message_id="message")
        with self.assertRaises(sqlite3.IntegrityError): self.msg_db.commit()
        self.msg_db.rollback()
        insert(self.msg_db,"conversation_read_markers", organization_id="org",conversation_id="dm",viewer_employee_id="reporter",last_read_message_id="message")
        insert(self.msg_db,"conversation_preferences", organization_id="org",conversation_id="dm",viewer_employee_id="reporter",muted_employee_id="ana",mute_mode="digest")
        self.msg_db.commit()

    def test_recipient_requires_membership_in_the_message_conversation(self):
        self.message(); self.msg_db.commit()
        self.msg_db.execute("DELETE FROM conversation_memberships WHERE conversation_id='dm' AND employee_id='reporter'")
        self.msg_db.commit()
        insert(self.msg_db,"message_recipient_states",organization_id="org",message_id="message",conversation_id="dm",employee_id="reporter",
               state="snoozed",updated_at="2026-11-15T12:00:00Z",owner_office_id="hq",origin_incarnation="i",origin_sequence=1,org_epoch=1,org_sequence=1)
        with self.assertRaises(sqlite3.IntegrityError): self.msg_db.commit()
        self.msg_db.rollback()

    def test_employee_status_is_one_enum_and_only_agents_pause(self):
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE employees SET status='paused' WHERE id='reporter'")
        self.db.rollback()
        self.db.execute("UPDATE employees SET status='paused' WHERE id='ana'")
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE employees SET status='offboarded' WHERE id='ana'")
        self.db.rollback()
        self.db.execute("UPDATE employees SET offboarded_at='2026-11-15T12:00:00Z',status='inactive' WHERE id='ana'")
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE employees SET status='active' WHERE id='ana'")
        self.db.rollback()
        columns = [row[1] for row in self.db.execute("PRAGMA table_xinfo(employees)")]
        self.assertNotIn("is_owner", columns)
        self.assertNotIn("agent_status", columns)

    def test_hq_created_records_always_have_a_number(self):
        for table, extra in (("teams", dict(name="No number")),
                             ("projects", dict(slug="no-number", name="No number", owner_team_id="payments")),
                             ("releases", dict(project_id="project", title="No number")),
                             ("milestones", dict(project_id="project", release_id="release", title="No number"))):
            with self.subTest(table=table), self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(
                    f"INSERT INTO {table} (organization_id,id,schema_version,revision,number,{','.join(extra)}) "
                    f"VALUES ('org','unnumbered',1,1,NULL,{','.join('?' for _ in extra)})", list(extra.values()))
            self.db.rollback()

    def test_task_numbers_are_minted_per_office_and_unique_in_the_organization(self):
        self.assertEqual(self.db.execute("SELECT key FROM tasks WHERE id='task'").fetchone()[0], "T-A42")
        base = dict(organization_id="org", schema_version=1, revision=1, team_id="payments", type="task",
                    title="t", status="todo")
        # the same number under another office's code is a different task, in any project or none
        insert(self.db, "tasks", **base, id="east-42", office_code="B", number=42)
        insert(self.db, "record_refs", organization_id="org", kind="task", id="east-42", is_deleted=0)
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT key FROM tasks WHERE id='east-42'").fetchone()[0], "T-B42")
        # two projectless tasks of one team may carry the same number under different office codes,
        # and a task may move out of its project into that team without changing its reference
        insert(self.db, "tasks", **base, id="team-a7", office_code="A", number=7)
        insert(self.db, "tasks", **base, id="team-b7", office_code="B", number=7)
        insert(self.db, "tasks", **base, id="proj-b8", office_code="B", number=8, project_id="project")
        for identity in ("team-a7", "team-b7", "proj-b8"):
            insert(self.db, "record_refs", organization_id="org", kind="task", id=identity, is_deleted=0)
        self.db.commit()
        self.db.execute("UPDATE tasks SET project_id=NULL WHERE id='proj-b8'")
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT key FROM tasks WHERE id='proj-b8'").fetchone()[0], "T-B8")
        # one office never gives the same number twice, whatever the project
        with self.assertRaises(sqlite3.IntegrityError):
            insert(self.db, "tasks", **base, id="dup", office_code="A", number=42)
        self.db.rollback()
        # a task always has its number, and its office code must be one HQ issued
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT INTO tasks (organization_id,id,schema_version,revision,office_code,number,team_id,title,status) "
                            "VALUES ('org','none',1,1,'A',NULL,'payments','t','todo')")
        self.db.rollback()
        insert(self.db, "tasks", **base, id="unknown-office", office_code="Z", number=1)
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.commit()
        self.db.rollback()
        # codes are one or two capital letters without I and O; an office has one current code
        for bad in ("I", "O", "a", "A1", "ABC", ""):
            with self.subTest(code=bad), self.assertRaises(sqlite3.IntegrityError):
                insert(self.db, "office_codes", organization_id="org", code=bad, office_id="numbering-a", issued_at="2026-11-15T09:00:00Z")
            self.db.rollback()
        with self.assertRaises(sqlite3.IntegrityError):
            insert(self.db, "office_codes", organization_id="org", code="C", office_id="numbering-a", issued_at="2026-11-16T09:00:00Z")
        self.db.rollback()
        self.db.execute("UPDATE office_codes SET retired_at='2026-11-16T09:00:00Z' WHERE code='B'")
        insert(self.db, "office_codes", organization_id="org", code="AA", office_id="numbering-b", issued_at="2026-11-16T09:00:00Z")
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT key FROM tasks WHERE id='east-42'").fetchone()[0], "T-B42")
        # the durable mark is never below the last number used
        insert(self.db, "office_task_counters", organization_id="org", office_code="A", last_number=42, reserved_through=100)
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE office_task_counters SET last_number=101")
        self.db.rollback()
        # HQ no longer keeps a task counter
        with self.assertRaises(sqlite3.IntegrityError):
            insert(self.db, "reference_counters", organization_id="org", scope_kind="project", scope_id="project",
                   entity_kind="task", last_number=1)
        self.db.rollback()

    def test_task_priority_is_a_closed_list_and_effort_is_a_whole_number_of_points(self):
        self.assertEqual(tuple(self.db.execute("SELECT priority, effort FROM tasks WHERE id='task'").fetchone()), ("normal", None))
        for priority in ("urgent", "high", "normal", "low"):
            self.db.execute("UPDATE tasks SET priority=?", (priority,))
        for effort in (0, 3, 13, None):
            self.db.execute("UPDATE tasks SET effort=?", (effort,))
        self.db.commit()
        for column, bad in (("priority", "critical"), ("priority", ""), ("effort", -1), ("effort", "large"), ("effort", 2.5)):
            with self.subTest(column=column, value=bad), self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(f"UPDATE tasks SET {column}=?", (bad,))
            self.db.rollback()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE tasks SET priority=NULL")
        self.db.rollback()

    def test_task_status_is_a_closed_list_ending_in_done(self):
        for status in ("todo", "in_progress", "blocked", "cancelled", "done"):
            self.db.execute("UPDATE tasks SET status=?", (status,))
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE tasks SET status='closed'")
        self.db.rollback()

    def test_milestone_and_release_are_done_when_their_tasks_are(self):
        common = dict(organization_id="org", schema_version=1, revision=1)
        insert(self.db, "releases", **common, id="with-milestone", project_id="project", title="Has a milestone")
        insert(self.db, "releases", **common, id="plain", project_id="project", title="No milestones")
        insert(self.db, "releases", **common, id="empty", project_id="project", title="Nothing in it")
        insert(self.db, "milestones", **common, id="m1", project_id="project", release_id="with-milestone", title="M")
        task = self.db.execute("SELECT id FROM tasks").fetchone()[0]
        self.db.execute("UPDATE tasks SET project_id='project', team_id='payments', milestone_id='m1', status='in_progress'")
        insert(self.db, "release_tasks", organization_id="org", release_id="plain", project_id="project", task_id=task)
        self.db.commit()
        done = lambda view, key, value: self.db.execute(
            f"SELECT is_done FROM {view} WHERE {key}=?", (value,)).fetchone()[0]
        self.assertEqual(done("milestone_progress", "milestone_id", "m1"), 0)
        self.assertEqual(done("release_progress", "release_id", "with-milestone"), 0)
        self.assertEqual(done("release_progress", "release_id", "plain"), 0)
        self.assertEqual(done("release_progress", "release_id", "empty"), 0)
        self.db.execute("UPDATE tasks SET status='done'")
        self.db.commit()
        self.assertEqual(done("milestone_progress", "milestone_id", "m1"), 1)
        self.assertEqual(done("release_progress", "release_id", "with-milestone"), 1)
        self.assertEqual(done("release_progress", "release_id", "plain"), 1)
        self.assertEqual(done("release_progress", "release_id", "empty"), 0)
        counts = self.db.execute("SELECT task_count, done_task_count FROM release_progress WHERE release_id='plain'").fetchone()
        self.assertEqual(tuple(counts), (1, 1))
        counts = self.db.execute("SELECT task_count, done_task_count FROM release_progress WHERE release_id='with-milestone'").fetchone()
        self.assertEqual(tuple(counts), (1, 1))  # counted from its milestones, not from a manifest
        # a cancelled task counts as done
        self.db.execute("UPDATE tasks SET status='cancelled'")
        self.db.commit()
        self.assertEqual(done("milestone_progress", "milestone_id", "m1"), 1)
        self.assertEqual(done("release_progress", "release_id", "plain"), 1)
        # a second milestone with no tasks keeps its release open
        insert(self.db, "milestones", **common, id="m2", project_id="project", release_id="with-milestone", title="Empty")
        self.db.commit()
        self.assertEqual(done("milestone_progress", "milestone_id", "m2"), 0)
        self.assertEqual(done("release_progress", "release_id", "with-milestone"), 0)
        # a manifest entry whose task no longer exists is ignored, so it cannot make a release done
        insert(self.db, "record_refs", organization_id="org", kind="task", id="gone", is_deleted=1)
        insert(self.db, "release_tasks", organization_id="org", release_id="empty", project_id="project", task_id="gone")
        self.db.commit()
        row = self.db.execute("SELECT task_count, is_done FROM release_progress WHERE release_id='empty'").fetchone()
        self.assertEqual(tuple(row), (0, 0))
        # a release with milestones AND an explicit release_tasks entry counts both (deduplicated)
        insert(self.db, "releases", **common, id="mixed", project_id="project", title="Mixed")
        insert(self.db, "milestones", **common, id="m-mixed", project_id="project", release_id="mixed", title="MM")
        seed_task(self.db, task="mixed-ms")
        seed_task(self.db, task="mixed-rt")
        self.db.execute("UPDATE tasks SET milestone_id='m-mixed', status='done' WHERE id='mixed-ms'")
        insert(self.db, "release_tasks", organization_id="org", release_id="mixed", project_id="project", task_id="mixed-ms")
        insert(self.db, "release_tasks", organization_id="org", release_id="mixed", project_id="project", task_id="mixed-rt")
        self.db.commit()
        row = self.db.execute("SELECT task_count, done_task_count, is_done FROM release_progress WHERE release_id='mixed'").fetchone()
        self.assertEqual(tuple(row), (2, 1, 0))
        self.db.execute("UPDATE tasks SET status='done' WHERE id='mixed-rt'")
        self.db.commit()
        row = self.db.execute("SELECT task_count, done_task_count, is_done FROM release_progress WHERE release_id='mixed'").fetchone()
        self.assertEqual(tuple(row), (2, 2, 1))

    def test_task_details_preserve_structured_payload_and_reject_bad_json(self):
        insert(self.db,"task_details",organization_id="org",task_id="task",what_happened="Duplicate",expected="One",steps="Retry",environment="staging",
               reopen_count=2,shortcut_json='{"type":"debt","ids":["DEBT-1"]}',done_anyway_json='{"reason":"Approved","steps":["step-1"]}')
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT json_extract(shortcut_json,'$.ids[0]'),reopen_count FROM task_details").fetchone()[0],"DEBT-1")
        with self.assertRaises(sqlite3.IntegrityError): self.db.execute("UPDATE task_details SET request_outcome_json='[]'")
        self.db.rollback()

    def test_checklist_sections_default_dod_and_attributed_final_summary(self):
        """TASK-88: per-section checklist positions, default DoD arrays, all-or-nothing summary."""
        columns = [row[1] for row in self.db.execute("PRAGMA table_info(task_details)")]
        self.assertNotIn("definition_of_done", columns)
        self.assertIn("done_anyway_json", columns)
        step = dict(organization_id="org", target_kind="task", target_id="task", required=1, done=0)
        for identity, section in (("ac", "acceptance"), ("dod", "dod"), ("work", "work")):
            insert(self.db, "checklist_steps", **step, id=identity, section=section, position=0, text=identity)
        insert(self.db, "checklist_steps", **step, id="default", position=1, text="Defaults to work")
        self.db.commit()
        self.assertEqual(self.db.execute(
            "SELECT section FROM checklist_steps WHERE id='default'").fetchone()[0], "work")
        for bad in (dict(id="dup", section="dod", position=0), dict(id="odd", section="notes", position=5)):
            with self.subTest(bad=bad), self.assertRaises(sqlite3.IntegrityError):
                insert(self.db, "checklist_steps", **step, text="x", **bad)
            self.db.rollback()

        self.db.execute("UPDATE organizations SET default_dod_json='[\"Tests pass\"]' WHERE id='org'")
        self.db.execute("UPDATE projects SET default_dod_json='[]' WHERE id='project'")
        self.db.commit()
        for statement in ("UPDATE organizations SET default_dod_json='{\"a\":1}'",
                          "UPDATE projects SET default_dod_json='not json'"):
            with self.subTest(statement=statement), self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(statement)
            self.db.rollback()

        self.db.execute("UPDATE tasks SET final_summary='Shipped the fix',final_summary_at='2026-11-27T00:00:00Z',"
                        "final_summary_by_kind='employee',final_summary_by_id='reporter' WHERE id='task'")
        self.db.commit()
        for statement in (
            "UPDATE tasks SET final_summary_by_id=NULL WHERE id='task'",
            "UPDATE tasks SET final_summary='   ' WHERE id='task'",
            "UPDATE tasks SET final_summary=printf('%.8001c', 'x') WHERE id='task'",
            "UPDATE tasks SET final_summary_by_kind='system' WHERE id='task'",
            "UPDATE tasks SET final_summary=NULL WHERE id='task'",
        ):
            with self.subTest(statement=statement), self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(statement)
                self.db.commit()
            self.db.rollback()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE tasks SET final_summary_by_id='nobody' WHERE id='task'")
            self.db.commit()
        self.db.rollback()
        self.db.execute("UPDATE tasks SET final_summary=NULL,final_summary_at=NULL,final_summary_by_kind=NULL,"
                        "final_summary_by_id=NULL WHERE id='task'")
        self.db.commit()

    def test_feature_replacement_must_stay_in_the_same_project(self):
        for identity,project in (("a","project"),("b","project")):
            insert(self.db,"features",organization_id="org",id=identity,schema_version=1,revision=1,project_id=project,local_slug=identity,title=identity,status="planned")
        self.db.commit()
        self.db.execute("UPDATE features SET replaced_by_feature_id='b',delivered_at='2026-11-15T12:00:00Z',retired_reason='Superseded' WHERE id='a'")
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError): self.db.execute("UPDATE features SET replaced_by_feature_id='a' WHERE id='a'")
        self.db.rollback()
        insert(self.db,"projects",organization_id="org",id="second",schema_version=1,revision=1,slug="second",name="Second",owner_team_id="payments")
        insert(self.db,"features",organization_id="org",id="c",schema_version=1,revision=1,project_id="second",local_slug="c",title="C",status="planned")
        self.db.commit()
        self.db.execute("UPDATE features SET replaced_by_feature_id='c' WHERE id='a'")
        with self.assertRaises(sqlite3.IntegrityError): self.db.commit()
        self.db.rollback()

    def test_command_retry_identity_and_terminal_outcome_are_retained(self):
        fields=dict(organization_id="org",command_id="command",origin_office_id="hq",actor_kind="employee",actor_id="ana",command_kind="task.create",
                    request_sha256="a"*64,state="reserved",created_at="2026-11-15T12:00:00Z",updated_at="2026-11-15T12:00:00Z")
        insert(self.db,"mutation_receipts",**fields); self.db.commit()
        for sql in ("UPDATE mutation_receipts SET request_sha256='"+"b"*64+"'", "DELETE FROM mutation_receipts", "INSERT OR REPLACE INTO mutation_receipts SELECT * FROM mutation_receipts"):
            with self.subTest(sql=sql), self.assertRaises(sqlite3.IntegrityError): self.db.execute(sql)
            self.db.rollback()
        self.db.execute("UPDATE mutation_receipts SET state='completed',result_ref='protected-result'")
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError): self.db.execute("UPDATE mutation_receipts SET state='reserved'")
        self.db.rollback()

    def test_origin_watermark_cannot_regress_fork_or_reopen(self):
        insert(self.db,"office_origin_streams",organization_id="org",office_id="hq",stream_kind="work",incarnation="i",high_water_sequence=2,journal_ref="journal",head_sha256="a"*64,state="sealed")
        self.db.commit()
        for sql in ("UPDATE office_origin_streams SET high_water_sequence=1", "UPDATE office_origin_streams SET head_sha256='"+"b"*64+"'", "UPDATE office_origin_streams SET state='open'", "INSERT OR REPLACE INTO office_origin_streams SELECT * FROM office_origin_streams"):
            with self.subTest(sql=sql), self.assertRaises(sqlite3.IntegrityError): self.db.execute(sql)
            self.db.rollback()

    def test_receipt_stream_and_checkpoint_are_registered_once_and_kept(self):
        insert(self.db, "mutation_receipts", organization_id="org", command_id="command", origin_office_id="hq",
               actor_kind="system", command_kind="task.create", request_sha256="a"*64, state="reserved",
               created_at="2026-11-15T12:00:00Z", updated_at="2026-11-15T12:00:00Z")
        stream = dict(organization_id="org", office_id="hq", incarnation="i", high_water_sequence=0,
                      journal_ref="journal", state="open")
        for kind in ("organization", "work", "message"):
            insert(self.db, "office_origin_streams", **stream, stream_kind=kind)
        insert(self.db, "office_recovery_checkpoints", organization_id="org", office_id="hq", hq_office_id="hq",
               authority_epoch=1, highest_seen_epoch=1, applied_sequence=5, digest_sha256="a"*64, base_sequence=0,
               base_digest_sha256="a"*64, protocol_level=1, restore_pending=0, awaiting_snapshot=0,
               native_sequence=0, journal_ref="journal", updated_at="2026-11-15T12:00:00Z")
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            insert(self.db, "office_origin_streams", **stream, stream_kind="chat")
        self.db.rollback()
        for table, delete_msg in (
            ("mutation_receipts", "receipt retention requires reviewed expiry/archive procedure"),
            ("office_origin_streams", "origin watermark must be retained"),
            ("office_recovery_checkpoints", "authority checkpoint must be retained"),
        ):
            with self.subTest(table=table, verb="INSERT"):
                assert_refused(self, self.db, "UNIQUE constraint failed", f"INSERT INTO {table} SELECT * FROM {table}")
            with self.subTest(table=table, verb="INSERT OR REPLACE"):
                assert_refused(self, self.db, delete_msg, f"INSERT OR REPLACE INTO {table} SELECT * FROM {table}")
        assert_refused(self, self.db, "origin watermark must be retained", "DELETE FROM office_origin_streams")
        assert_refused(self, self.db, "authority checkpoint must be retained", "DELETE FROM office_recovery_checkpoints")
        # A checkpoint only moves forward and never forks at the same position.
        self.db.execute("UPDATE office_recovery_checkpoints SET applied_sequence=6,digest_sha256=?", ("b"*64,))
        self.db.commit()
        for change in ("applied_sequence=5", "digest_sha256='" + "c"*64 + "'", "office_id='numbering-a'"):
            with self.subTest(change=change):
                assert_refused(self, self.db, "recovery checkpoint regressed or forked",
                               f"UPDATE office_recovery_checkpoints SET {change}")

    def test_one_automatic_channel_per_organization_team_and_project(self):
        def channel(identity, scope, **target):
            insert(self.msg_db, "conversations", organization_id="org", id=identity, kind="channel", auto_scope=scope,
                   created_at="2026-11-15T12:00:00Z", **target)
        channel("everyone", "organization")
        channel("payments", "team", team_id="payments")
        channel("platform", "team", team_id="platform")
        channel("project", "project", project_id="project")
        self.msg_db.commit()
        for identity, scope, target in (("everyone-2", "organization", {}), ("payments-2", "team", {"team_id": "payments"}),
                                        ("project-2", "project", {"project_id": "project"})):
            with self.subTest(scope=scope):
                assert_refused(self, self.msg_db, "UNIQUE constraint failed: conversations",
                               lambda: channel(identity, scope, **target))

    def test_an_organization_has_one_hq(self):
        insert(self.db, "offices", organization_id="org", id="east", slug="east", schema_version=1, revision=1,
               name="East", role="satellite")
        self.db.commit()
        assert_refused(self, self.db, "UNIQUE constraint failed: offices", lambda: insert(
            self.db, "offices", organization_id="org", id="hq-2", slug="hq-2", schema_version=1, revision=1,
            name="Second HQ", role="hq"))
        assert_refused(self, self.db, "UNIQUE constraint failed: offices", "UPDATE offices SET role='hq' WHERE id='east'")

    def test_the_built_in_hr_lead_is_one_agent_without_the_hiring_permission(self):
        hr = dict(organization_id="org", schema_version=1, revision=1, kind="agent", home_team_id="payments",
                  builtin_key="hr", may_hire=0)
        for bad in (dict(kind="person"), dict(may_hire=1), dict(builtin_key="ceo")):
            with self.subTest(bad=bad):
                assert_refused(self, self.db, "CHECK constraint failed", lambda: insert(
                    self.db, "employees", **(hr | bad), id="bad", handle="bad", name="Bad"))
        insert(self.db, "employees", **hr, id="hr", handle="hr", name="HR")
        self.db.commit()
        assert_refused(self, self.db, "UNIQUE constraint failed: employees", lambda: insert(
            self.db, "employees", **hr, id="hr-2", handle="hr-2", name="HR 2"))

    def test_the_built_in_chief_of_staff_is_one_agent_that_may_hold_the_hiring_permission(self):
        cos = dict(organization_id="org", schema_version=1, revision=1, kind="agent", home_team_id="payments",
                   builtin_key="chief_of_staff")
        for bad in (dict(kind="person"), dict(builtin_key="ceo"), dict(builtin_key="Chief_Of_Staff"),
                    dict(builtin_key="")):
            with self.subTest(bad=bad):
                assert_refused(self, self.db, "CHECK constraint failed", lambda: insert(
                    self.db, "employees", **(cos | bad), id="bad", handle="bad", name="Bad"))
        # Unlike the HR lead, the Chief of Staff may hold may_hire (the owner turns it off and on).
        insert(self.db, "employees", **cos, id="tess", handle="tess", name="Tess", may_hire=1)
        self.db.commit()
        self.db.execute("UPDATE employees SET may_hire=0 WHERE id='tess'")
        self.db.execute("UPDATE employees SET may_hire=1 WHERE id='tess'")
        self.db.commit()
        for may_hire in (0, 1):
            with self.subTest(may_hire=may_hire):
                assert_refused(self, self.db, "UNIQUE constraint failed: employees", lambda: insert(
                    self.db, "employees", **cos, id="tess-2", handle="tess-2", name="Tess 2", may_hire=may_hire))
        # One of each built-in lives beside the other: the two indexes are separate.
        insert(self.db, "employees", organization_id="org", schema_version=1, revision=1, kind="agent",
               home_team_id="payments", builtin_key="hr", may_hire=0, id="hr", handle="hr", name="HR")
        self.db.commit()
        assert_refused(self, self.db, "UNIQUE constraint failed: employees",
                       "UPDATE employees SET builtin_key='chief_of_staff' WHERE id='hr'")

    def test_an_affected_feature_is_in_the_task_project(self):
        feature = dict(organization_id="org", schema_version=1, revision=1, title="F", status="planned")
        insert(self.db, "features", **feature, id="here", project_id="project", local_slug="here")
        insert(self.db, "projects", organization_id="org", id="second", schema_version=1, revision=1,
               slug="second", name="Second", owner_team_id="payments")
        insert(self.db, "features", **feature, id="elsewhere", project_id="second", local_slug="elsewhere")
        self.db.commit()
        message = "affected feature belongs to another project"
        assert_refused(self, self.db, message, lambda: insert(
            self.db, "task_details", organization_id="org", task_id="task", affects_feature_id="elsewhere"))
        insert(self.db, "task_details", organization_id="org", task_id="task", affects_feature_id="here")
        self.db.commit()
        assert_refused(self, self.db, message, "UPDATE task_details SET affects_feature_id='elsewhere'")
        # The task keeps the feature's project while the feature is named.
        for project in ("'second'", "NULL"):
            with self.subTest(project=project):
                assert_refused(self, self.db, message, f"UPDATE tasks SET project_id={project} WHERE id='task'")
        self.db.execute("UPDATE task_details SET affects_feature_id=NULL")
        self.db.execute("UPDATE tasks SET project_id='second' WHERE id='task'")
        self.db.execute("UPDATE task_details SET affects_feature_id='elsewhere'")
        self.db.commit()
        # Details may be written before their task in one transaction; the rule applies once both exist.
        insert(self.db, "task_details", organization_id="org", task_id="later", affects_feature_id="here")
        seed_task(self.db, task="later")  # in "project", like the feature; seed_task commits

    def test_feature_status_is_a_closed_list(self):
        insert(self.db, "features", organization_id="org", id="f", schema_version=1, revision=1, project_id="project",
               local_slug="f", title="F", status="live", owner_employee_id="reporter")
        self.db.commit()
        for status in ("planned", "in_progress", "retired"):
            self.db.execute("UPDATE features SET status=?", (status,))
        self.db.commit()
        assert_refused(self, self.db, "CHECK constraint failed", "UPDATE features SET status='shipped'")

    def test_a_tombstoned_task_can_still_be_named_as_a_duplicate_or_a_linked_step(self):
        seed_task(self.db, task="original")
        seed_task(self.db, task="copy")
        insert(self.db, "task_details", organization_id="org", task_id="copy", duplicate_of_task_id="original",
               closed_reason="duplicate")
        insert(self.db, "checklist_steps", organization_id="org", target_kind="task", target_id="task", id="step",
               position=0, text="Fix the original", required=1, done=0, linked_task_id="original")
        self.db.commit()
        insert(self.db, "tombstones", organization_id="org", kind="task", id="original", schema_version=1,
               revision=1, terminal_state="done")
        self.db.execute("UPDATE record_refs SET is_deleted=1 WHERE id='original'")
        self.db.execute("DELETE FROM tasks WHERE id='original'")
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT duplicate_of_task_id FROM task_details").fetchone()[0], "original")
        self.assertEqual(self.db.execute("SELECT linked_task_id FROM checklist_steps").fetchone()[0], "original")
        # A task that never existed still cannot be named.
        for statement in ("UPDATE task_details SET merged_into_task_id='missing'",
                          "UPDATE checklist_steps SET linked_task_id='missing'"):
            with self.subTest(statement=statement), self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(statement)
                self.db.commit()
            self.db.rollback()


class BudgetSchemaTests(unittest.TestCase):
    def setUp(self):
        self.db = database("budgets-schema.sql")
        self.addCleanup(self.db.close)
        seed_budget_mirrors(self.db)

    def seed_budget_task(self, task="second"):
        insert(self.db, "budget_tasks", organization_id="org", kind="task", id=task, is_deleted=0)
        self.db.commit()

    def budget(self, identity="budget", scope="task", period="lifetime", commit=True, **overrides):
        target={"task":"task_id", "agent":"agent_id", "release":"release_id"}[scope]
        value={"task":"task", "agent":"ana", "release":"release"}[scope]
        insert(self.db,"budgets",**(dict(organization_id="org",id=identity,current_revision=1,scope_kind=scope,
               period_kind=period,time_zone="UTC",time_zone_source="hq",created_at="2026-11-15T00:00:00Z",**{target:value}) | overrides))
        insert(self.db,"budget_policy_versions",organization_id="org",budget_id=identity,revision=1,state="active",unit="tokens",limit_amount=100,
               changed_by_id="reporter",changed_at="2026-11-15T00:00:00Z")
        if commit:
            self.db.commit()

    def period(self, budget="budget", identity="period"):
        insert(self.db,"budget_periods",organization_id="org",budget_id=budget,id=identity,policy_revision=1,period_key="lifetime",state="open")
        self.db.commit()

    def hold(self, budget="budget", period="period"):
        insert(self.db,"budget_holds",organization_id="org",budget_id=budget,period_id=period,policy_revision=1,stage="base",state="held",
               created_at="2026-11-15T00:00:00Z",updated_at="2026-11-15T00:00:00Z")

    def release(self):
        insert(self.db,"budget_releases",organization_id="org",id="release")
        self.db.commit()

    def test_one_budget_per_task_and_per_release(self):
        self.release()
        self.seed_budget_task("second")
        self.budget()
        self.budget("second", task_id="second")
        self.budget("cap", scope="release")
        assert_refused(self, self.db, "UNIQUE constraint failed: budgets", lambda: self.budget("again"))
        assert_refused(self, self.db, "UNIQUE constraint failed: budgets", lambda: self.budget("cap-again", scope="release"))

    def test_a_budget_keeps_its_scope_and_never_lowers_its_revision(self):
        self.seed_budget_task("second")
        self.budget()
        message = "budget scope or revision changed unsafely"
        for change in ("task_id='second'", "time_zone='Europe/Paris'", "time_zone_source='owner_person',time_zone_subject_ref='reporter'"):
            with self.subTest(change=change):
                assert_refused(self, self.db, message, f"UPDATE budgets SET {change}")
        insert(self.db,"budget_policy_versions",organization_id="org",budget_id="budget",revision=2,state="active",unit="tokens",limit_amount=200,
               changed_by_id="reporter",changed_at="2026-11-15T01:00:00Z")
        self.db.execute("UPDATE budgets SET current_revision=2")
        self.db.commit()
        assert_refused(self, self.db, message, "UPDATE budgets SET current_revision=1")

    def test_owner_timezone_change_advances_policy_revision_and_preserves_prior_periods(self):
        self.budget("daily", "agent", "day")
        insert(self.db, "budget_periods", organization_id="org", budget_id="daily", id="p-utc",
               policy_revision=1, period_key="2026-11-15", starts_at="2026-11-15T00:00:00Z",
               ends_at="2026-11-16T00:00:00Z", state="closed")
        self.db.commit()
        # Changing timezone with a new immutable policy revision succeeds while keeping p-utc intact.
        insert(self.db, "budget_policy_versions", organization_id="org", budget_id="daily", revision=2,
               state="active", unit="tokens", limit_amount=100, time_zone="America/New_York",
               time_zone_source="owner_person", time_zone_subject_ref="reporter",
               changed_by_id="reporter", changed_at="2026-11-16T00:00:00Z", reason="Owner moved timezone")
        self.db.execute(
            "UPDATE budgets SET current_revision=2, time_zone='America/New_York', "
            "time_zone_source='owner_person', time_zone_subject_ref='reporter' WHERE id='daily'"
        )
        insert(self.db, "budget_periods", organization_id="org", budget_id="daily", id="p-ny",
               policy_revision=2, period_key="2026-11-16", starts_at="2026-11-16T05:00:00Z",
               ends_at="2026-11-17T05:00:00Z", state="open")
        self.db.commit()
        row = self.db.execute(
            "SELECT current_revision, time_zone, time_zone_source FROM budgets WHERE id='daily'"
        ).fetchone()
        self.assertEqual(tuple(row), (2, "America/New_York", "owner_person"))
        prior = self.db.execute(
            "SELECT starts_at, ends_at FROM budget_periods WHERE id='p-utc'"
        ).fetchone()
        self.assertEqual(tuple(prior), ("2026-11-15T00:00:00Z", "2026-11-16T00:00:00Z"))
        # Host account IDs (outside org employees) are rejected by FK (L-02).
        with self.assertRaises(sqlite3.IntegrityError):
            insert(self.db, "budget_policy_versions", organization_id="org", budget_id="daily", revision=3,
                   state="active", unit="tokens", limit_amount=100, time_zone="Europe/London",
                   time_zone_source="owner_person", time_zone_subject_ref="host-account-outside-org",
                   changed_by_id="reporter", changed_at="2026-11-17T00:00:00Z")
            self.db.commit()
        self.db.rollback()
        # org_epoch = 0 on messages is rejected (L-02) in isolated messaging-schema.sql.
        msg_db = database("messaging-schema.sql")
        self.addCleanup(msg_db.close)
        seed_messaging_mirrors(msg_db)
        insert(msg_db, "conversations", organization_id="org", id="c-epoch", kind="dm", created_at="2026-11-15T00:00:00Z")
        msg_db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            insert(msg_db, "messages", organization_id="org", id="m-epoch-0", conversation_id="c-epoch",
                   sender_kind="employee", sender_employee_id="reporter", sender_office_id="numbering-a",
                   sender_display_name="Reporter", kind="message", priority="normal", body_markdown="hi",
                   created_at="2026-11-15T00:00:00Z", origin_office_id="numbering-a", origin_incarnation="i",
                   origin_sequence=1, org_epoch=0, org_sequence=1, content_sha256="a"*64)
        msg_db.rollback()

    def test_budget_history_is_never_deleted_and_events_never_change(self):
        self.budget();self.period()
        insert(self.db,"budget_events",organization_id="org",id="event",budget_id="budget",period_id="period",operation_id="operation",
               kind="evaluated",policy_revision=1,actor_kind="system",occurred_at="2026-11-15T00:00:00Z",payload_json="{}")
        insert(self.db,"budget_notices",organization_id="org",id="notice",budget_id="budget",period_id="period",policy_revision=1,stage="base",
               threshold=100,recipient_kind="owner",kind="budget_reached",delivery_state="pending",created_at="2026-11-15T00:00:00Z")
        self.hold()
        self.db.commit()
        self.db.execute("UPDATE budget_holds SET state='released',release_reason='Extended'")  # a task hold may change
        self.db.commit()
        assert_refused(self, self.db, "budget history is immutable", "UPDATE budget_events SET kind='warning'")
        for table in ("budget_holds", "budget_notices", "budget_events", "budget_periods", "budgets"):
            with self.subTest(table=table):
                assert_refused(self, self.db, "budget identity/history must be retained", f"DELETE FROM {table}")

    def test_a_hold_cannot_be_moved_onto_a_release_cap(self):
        self.release()
        self.budget()
        self.budget("cap", scope="release")
        self.period()
        self.period("cap", "cap-period")
        self.hold()
        self.db.commit()
        assert_refused(self, self.db, "release caps do not stop work",
                       "UPDATE budget_holds SET budget_id='cap',period_id='cap-period'")

    def test_an_agent_budget_keeps_its_employee_an_agent(self):
        self.budget("daily","agent","day")
        assert_refused(self, self.db, "budgeted employee must remain an agent", "UPDATE budget_employees SET kind='person' WHERE id='ana'")
        employee = dict(organization_id="org", id="newcomer")
        # A budget may be written before its employee mirror in one transaction; the employee must be an agent.
        self.budget("early","agent","day",commit=False,agent_id="newcomer")
        assert_refused(self, self.db, "budgeted employee must be an agent",
                       lambda: insert(self.db, "budget_employees", **employee, kind="person"))
        self.budget("early","agent","day",commit=False,agent_id="newcomer")
        insert(self.db, "budget_employees", **employee, kind="agent")
        self.db.commit()

    def test_an_early_warning_is_only_for_the_base_limit(self):
        self.budget();self.period()
        fields=dict(organization_id="org",budget_id="budget",period_id="period",policy_revision=1,threshold=80,
                    recipient_kind="owner",kind="neutral",delivery_state="pending",created_at="2026-11-15T00:00:00Z")
        insert(self.db,"budget_notices",**fields,id="warning",stage="base")
        self.db.commit()
        assert_refused(self, self.db, "CHECK constraint failed",
                       lambda: insert(self.db,"budget_notices",**fields,id="late",stage="extended"))

    def test_daily_and_monthly_agent_limits_coexist_but_duplicate_scope_does_not(self):
        self.budget("daily","agent","day")
        self.budget("monthly","agent","month")
        with self.assertRaises(sqlite3.IntegrityError): self.budget("another","agent","day")
        self.db.rollback()
        with self.assertRaises(sqlite3.IntegrityError): self.budget("person","agent","day",agent_id="reporter")
        self.db.rollback()

    def test_policy_history_is_immutable_and_current_revision_requires_a_version(self):
        self.budget()
        for sql in ("UPDATE budget_policy_versions SET limit_amount=500", "INSERT OR REPLACE INTO budget_policy_versions SELECT * FROM budget_policy_versions", "DELETE FROM budget_policy_versions"):
            with self.subTest(sql=sql),self.assertRaises(sqlite3.IntegrityError): self.db.execute(sql)
            self.db.rollback()
        self.db.execute("UPDATE budgets SET current_revision=2")
        with self.assertRaises(sqlite3.IntegrityError): self.db.commit()
        self.db.rollback()

    def test_extension_cannot_be_reset_by_replay_but_new_policy_can_rearm_it(self):
        self.budget();self.period()
        self.db.execute("UPDATE budget_periods SET extension_used=1");self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError): self.db.execute("UPDATE budget_periods SET extension_used=0")
        self.db.rollback()
        insert(self.db,"budget_policy_versions",organization_id="org",budget_id="budget",revision=2,state="active",unit="tokens",limit_amount=200,
               changed_by_id="reporter",changed_at="2026-11-15T01:00:00Z")
        self.db.execute("UPDATE budgets SET current_revision=2")
        self.db.execute("UPDATE budget_periods SET policy_revision=2,extension_used=0,used_amount=120")
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT used_amount FROM budget_periods").fetchone()[0],120)

    def test_notice_delivery_dedupes_each_threshold_and_extension_stage(self):
        self.budget();self.period()
        fields=dict(organization_id="org",id="notice",budget_id="budget",period_id="period",policy_revision=1,stage="base",threshold=100,
                    recipient_kind="owner",kind="budget_reached",delivery_state="pending",created_at="2026-11-15T00:00:00Z")
        insert(self.db,"budget_notices",**fields);self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError): insert(self.db,"budget_notices",**(fields | {"id":"duplicate"}))
        self.db.rollback()
        insert(self.db,"budget_notices",**(fields | {"id":"extended","stage":"extended"}));self.db.commit()
        self.assertEqual(self.db.execute("SELECT count(*) FROM budget_notices").fetchone()[0],2)

    def test_zero_limit_and_wrong_target_period_are_refused(self):
        self.budget()
        with self.assertRaises(sqlite3.IntegrityError):
            insert(self.db,"budget_policy_versions",organization_id="org",budget_id="budget",revision=2,state="active",unit="micro_usd",limit_amount=0,
                   changed_by_id="reporter",changed_at="2026-11-15T01:00:00Z")
        self.db.rollback()
        with self.assertRaises(sqlite3.IntegrityError): self.budget("invalid","task","day")
        self.db.rollback()

    def test_release_cap_cannot_acquire_a_stop_hold(self):
        self.release()
        self.budget(scope="release");self.period()
        with self.assertRaises(sqlite3.IntegrityError):
            insert(self.db,"budget_holds",organization_id="org",budget_id="budget",period_id="period",policy_revision=1,stage="base",state="held",
                   created_at="2026-11-15T00:00:00Z",updated_at="2026-11-15T00:00:00Z")
        self.db.rollback()


class AttachmentSchemaTests(unittest.TestCase):
    """TASK-98.1 (ADR-0037): one polymorphic, metadata-only attachments table in office.db; rich
    Day-1 metadata on message and ask-answer attachments; the human-private space isolated."""

    SHA = "a" * 64

    def setUp(self):
        self.db = database()
        self.addCleanup(self.db.close)
        seed_organization(self.db)

    def attachment(self, identity="file", **overrides):
        fields = dict(organization_id="org", id=identity, target_kind="project", target_id="project",
                      content_sha256=self.SHA, mime_type="application/pdf", byte_count=10,
                      filename="spec.pdf", title="Spec", artifact_status="final", page_count=3,
                      uploaded_by_employee_id="ana", created_at="2026-11-26T00:00:00Z",
                      updated_at="2026-11-26T00:00:00Z")
        insert(self.db, "attachments", **(fields | overrides))
        self.db.commit()

    def refused(self, **overrides):
        with self.assertRaises(sqlite3.IntegrityError, msg=str(overrides)):
            self.attachment("bad", **overrides)
        self.db.rollback()

    def test_attachments_are_strict_metadata_only_and_trigger_free(self):
        sql = self.db.execute("SELECT sql FROM sqlite_schema WHERE name = 'attachments'").fetchone()[0]
        self.assertTrue(sql.rstrip().endswith("STRICT"))
        columns = {row[1]: row[2] for row in self.db.execute("PRAGMA table_xinfo(attachments)")}
        self.assertEqual(list(columns), [
            "organization_id", "id", "target_kind", "target_id", "content_sha256", "mime_type",
            "byte_count", "filename", "title", "description", "artifact_status", "width", "height",
            "page_count", "uploaded_by_employee_id", "created_at", "updated_at"])
        self.assertNotIn("BLOB", columns.values(), "bytes never live in a database")
        self.assertEqual(self.db.execute(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = 'attachments'").fetchone()[0], 0)
        indexes = {row[0] for row in self.db.execute(
            "SELECT name FROM sqlite_schema WHERE type = 'index' AND tbl_name = 'attachments' AND sql IS NOT NULL")}
        self.assertEqual(indexes, {"attachments_by_target", "attachments_by_uploader", "attachments_by_content"})
        # The human-private space never appears in the shared schema.
        shared = (ROOT / "schema.sql").read_text()
        self.assertNotIn("user_private", shared)
        self.assertNotIn("user_space", shared)

    def test_every_shared_target_kind_and_status_is_accepted(self):
        for n, kind in enumerate(("task", "task_comment", "review_finding", "review_finding_reply", "project")):
            self.attachment(f"k{n}", target_kind=kind, target_id=f"target-{n}")
        self.attachment("org-wide", target_kind="organization", target_id=None)
        for n, status in enumerate(("draft", "review", "final", "superseded", "archived")):
            self.attachment(f"s{n}", artifact_status=status)
        insert(self.db, "attachments", organization_id="org", id="defaulted", target_kind="organization",
               content_sha256=self.SHA, mime_type="text/plain", byte_count=0, filename="a.txt", title="A",
               uploaded_by_employee_id="reporter", created_at="t", updated_at="t")
        self.assertEqual(self.db.execute(
            "SELECT artifact_status FROM attachments WHERE id = 'defaulted'").fetchone()[0], "final")
        # The same bytes bound twice are two rows, each its own attachment.
        self.attachment("copy-1")
        self.attachment("copy-2")
        self.assertEqual(self.db.execute(
            "SELECT COUNT(*) FROM attachments WHERE content_sha256 = ?", (self.SHA,)).fetchone()[0], 14)

    def test_attachment_rows_break_no_contract(self):
        for overrides in (
            {"target_kind": "user_private", "target_id": "reporter"},
            {"target_kind": "message"},
            {"target_kind": "organization"},
            {"target_kind": "project", "target_id": None},
            {"content_sha256": "A" * 64},
            {"content_sha256": "a" * 63},
            {"mime_type": "text/html"},
            {"mime_type": "image/svg+xml"},
            {"mime_type": "application/octet-stream"},
            {"byte_count": -1},
            {"filename": ""},
            {"title": ""},
            {"title": "t" * 161},
            {"description": "d" * 281},
            {"artifact_status": "published"},
            {"width": 0},
            {"page_count": 0},
            {"organization_id": "another"},
        ):
            with self.subTest(overrides=overrides):
                self.refused(**overrides)
        self.attachment("bounds", title="t" * 160, description="d" * 280, width=1, height=1)

    def test_message_and_answer_attachments_carry_day_one_metadata(self):
        msg = database("messaging-schema.sql")
        self.addCleanup(msg.close)
        columns = [row[1] for row in msg.execute("PRAGMA table_xinfo(message_attachments)")]
        for column in ("title", "description", "artifact_status", "page_count", "uploaded_by_employee_id"):
            self.assertIn(column, columns)
        answer = [row[1] for row in msg.execute("PRAGMA table_xinfo(message_answer_attachments)")]
        self.assertEqual(answer[:7], ["organization_id", "message_id", "employee_id", "id", "media_type",
                                      "byte_count", "content_sha256"])
        for column in ("title", "description", "artifact_status", "page_count", "uploaded_by_employee_id"):
            self.assertIn(column, answer)
        sql = msg.execute("SELECT sql FROM sqlite_schema WHERE name = 'message_answer_attachments'").fetchone()[0]
        self.assertTrue(sql.rstrip().endswith("STRICT"))
        msg.execute("PRAGMA foreign_keys = ON")
        # The rows the messaging adapter writes today still fit: every new column has a default.
        msg.execute("INSERT INTO message_attachments (organization_id, message_id, id, content_ref, media_type, "
                    "byte_count, content_sha256) VALUES ('org', 'm', 'a', 'ref', 'text/plain', 1, ?)", (self.SHA,))
        self.assertEqual(msg.execute("SELECT artifact_status FROM message_attachments").fetchone()[0], "final")
        msg.rollback()
        for column, value in (("title", "t" * 161), ("description", "d" * 281), ("artifact_status", "x"), ("page_count", 0)):
            with self.subTest(column=column), self.assertRaises(sqlite3.IntegrityError):
                msg.execute(f"INSERT INTO message_answer_attachments (organization_id, message_id, employee_id, id, "
                            f"media_type, byte_count, content_sha256, created_at, {column}) "
                            f"VALUES ('org', 'm', 'e', 'a', 'text/plain', 1, ?, 't', ?)", (self.SHA, value))
            msg.rollback()

    def test_the_private_space_is_its_own_database_keyed_by_owner_and_command(self):
        private = database("user-space-schema.sql")
        self.addCleanup(private.close)
        tables = {row[0] for row in private.execute("SELECT name FROM sqlite_schema WHERE type = 'table'")}
        self.assertEqual(tables, {"user_space_files"})
        shared_tables = {row[0] for row in self.db.execute("SELECT name FROM sqlite_schema WHERE type = 'table'")}
        self.assertNotIn("user_space_files", shared_tables)

        def row(identity, operation, **overrides):
            fields = dict(organization_id="org", id=identity, owner_employee_id="reporter",
                          operation_id=operation, operation_fingerprint="b" * 64,
                          content_sha256=self.SHA, mime_type="image/png",
                          byte_count=5, filename="me.png", title="Me", width=4, height=4,
                          created_at="t", updated_at="t")
            insert(private, "user_space_files", **(fields | overrides))

        row("one", "op-1")
        row("two", "op-2", description=None)
        # A command id is scoped to its owner: another person's same id is their own.
        row("seven", "op-1", owner_employee_id="someone-else")
        for identity, operation, overrides in (
            ("three", "op-1", {}),
            ("one", "op-3", {}),
            ("four", "op-4", {"mime_type": "text/html"}),
            ("five", "op-5", {"title": "t" * 161}),
            ("six", "op-6", {"artifact_status": "secret"}),
            ("eight", "op-8", {"operation_fingerprint": "B" * 64}),
            ("nine", "op-9", {"operation_fingerprint": "b" * 63}),
            ("ten", "op-10", {"operation_fingerprint": None}),
        ):
            with self.subTest(identity=identity), self.assertRaises(sqlite3.IntegrityError):
                row(identity, operation, **overrides)
        self.assertEqual(private.execute("SELECT COUNT(*) FROM sqlite_schema WHERE type = 'trigger'").fetchone()[0], 0)


class ProcessPackageAndDocumentationTests(unittest.TestCase):
    """Document 18: process packages, the documentation tree, issued versions and convergence."""

    SHA = "a" * 64

    def setUp(self):
        self.db = database()
        self.addCleanup(self.db.close)
        seed_organization(self.db)

    def package(self, package="dss", **fields):
        values = dict(organization_id="org", id=package, key=package, schema_version=1, revision=1,
                      origin="built_in", name=package, status="current")
        values.update(fields)
        insert(self.db, "development_processes", **values)

    def document(self, document="spec", **fields):
        values = dict(organization_id="org", id=document, schema_version=1, revision=1,
                      project_id="project", kind="product-dss", title="User authentication",
                      path=f"product/{document}.md")
        values.update(fields)
        self.db.execute(
            "INSERT OR IGNORE INTO record_refs(organization_id, kind, id, is_deleted) VALUES (?, 'document', ?, 0)",
            (values["organization_id"], values["id"]),
        )
        insert(self.db, "documents", **values)

    def issue(self, document="spec", version=1, revision=1, project_id="project", register_ref=True):
        if register_ref:
            self.db.execute(
                "INSERT OR IGNORE INTO record_refs(organization_id, kind, id, is_deleted) VALUES ('org', 'document', ?, 0)",
                (document,),
            )
        insert(self.db, "document_versions", organization_id="org", document_id=document,
               project_id=project_id, version=version, revision=revision, title="User authentication",
               body_sha256=self.SHA, body_markdown="## Purpose", issued_at="2026-11-18T09:00:00Z",
               issued_by_kind="employee", issued_by_id="reporter")

    def refused(self, statement):
        with self.assertRaises(sqlite3.IntegrityError):
            statement()
            self.db.commit()
        self.db.rollback()

    def test_any_employee_can_be_given_the_permission_to_hire(self):
        self.assertEqual(self.db.execute("SELECT may_hire FROM employees WHERE id='ana'").fetchone()[0], 0)
        self.db.execute("UPDATE employees SET may_hire=1 WHERE id IN ('ana', 'reporter')")  # agent, person
        self.db.commit()
        self.refused(lambda: self.db.execute("UPDATE employees SET may_hire=2 WHERE id='ana'"))

    def hire(self, request="hire", **fields):
        values = dict(organization_id="org", id=request, schema_version=1, revision=1, state="drafting",
                      requested_by_employee_id="reporter", home_team_id="payments",
                      created_at="2026-11-18T09:00:00Z", updated_at="2026-11-18T09:00:00Z")
        values.update(fields)
        insert(self.db, "hire_requests", **values)

    def test_a_hire_request_records_who_hired_when_it_was_confirmed_and_why_it_closed(self):
        confirmed = dict(confirmed_at="2026-11-18T10:00:00Z")
        self.hire()
        self.hire("hiring", state="hiring", failed_step="register with messaging", **confirmed)
        insert(self.db, "employees", organization_id="org", id="marlowe", handle="marlowe",
               schema_version=1, revision=1, kind="agent", name="Marlowe", home_team_id="payments")
        self.hire("hired", state="hired", employee_id="marlowe", **confirmed)
        self.hire("withdrawn", state="withdrawn", closed_reason="no longer needed")
        self.hire("failed", state="failed", failed_step="home", closed_reason="given up", **confirmed)
        self.db.commit()
        for fields in (dict(state="hiring"),                                     # hiring before it is confirmed
                       dict(state="drafting", **confirmed),                      # confirmed while still drafting
                       dict(state="hired", **confirmed),                         # hired without an employee
                       dict(state="drafting", employee_id="marlowe"),            # an employee before the hire
                       dict(state="drafting", failed_step="home"),               # a failed step before provisioning
                       dict(state="withdrawn"),                                  # withdrawn without a reason
                       dict(state="failed", closed_reason="", **confirmed),      # failed with an empty reason
                       dict(state="drafting", home_team_id="missing"),
                       dict(state="drafting", requested_by_employee_id="missing"),
                       dict(state="waiting"),                                    # no approval states remain
                       dict(state="approved", **confirmed)):
            self.refused(lambda: self.hire("bad", **fields))
        self.refused(lambda: self.hire("twice", state="hired", employee_id="marlowe", **confirmed))

    def test_a_task_is_in_the_releases_of_its_milestone_and_of_release_contents(self):
        for release in ("r1", "r2"):
            insert(self.db, "releases", organization_id="org", id=release, schema_version=1, revision=1,
                   project_id="project", title=release)
        insert(self.db, "milestones", organization_id="org", id="m1", schema_version=1, revision=1,
               project_id="project", release_id="r1", title="M1")
        for task in ("planned", "listed", "ktlo"):
            seed_task(self.db, task=task)
        self.db.execute("UPDATE tasks SET milestone_id='m1' WHERE id='planned'")
        insert(self.db, "release_tasks", organization_id="org", release_id="r2", project_id="project",
               task_id="listed")
        insert(self.db, "release_tasks", organization_id="org", release_id="r1", project_id="project",
               task_id="planned")  # also listed in its own release: counted once
        rows = sorted(tuple(r) for r in self.db.execute("SELECT task_id, release_id FROM task_releases"))
        self.assertEqual(rows, [("listed", "r2"), ("planned", "r1")])  # "ktlo" has no release lane

    def test_a_turn_records_what_started_it(self):
        columns = {row[1]: row for row in self.db.execute("PRAGMA table_info(agent_execution_turns)")}
        self.assertEqual(columns["started_by_kind"][4], "'run'")
        self.assertIn("started_by_employee_id", columns)
        self.assertIn("started_by_ref", columns)
        sql = self.db.execute("SELECT sql FROM sqlite_schema WHERE name='agent_execution_turns'").fetchone()[0]
        for kind in ("run", "message", "mention", "ask", "handoff", "assignment", "answer", "resume", "recovery", "background_wake"):
            self.assertIn(f"'{kind}'", sql)
        cfg_cols = {row[1]: row for row in self.db.execute("PRAGMA table_info(agent_execution_configs)")}
        self.assertEqual(cfg_cols["permission_preset"][4], "'safe_ask'")
        cfg_sql = self.db.execute("SELECT sql FROM sqlite_schema WHERE name='agent_execution_configs'").fetchone()[0]
        for preset in ("safe_ask", "read_only", "workspace_edits", "autonomous_sandboxed"):
            self.assertIn(f"'{preset}'", cfg_sql)
        self.assertNotIn("unrestricted", cfg_sql)
        self.assertNotIn("bypass", cfg_sql)

    def test_satellite_attempt_rebuild_succeeds_without_hq_permit_rows_and_in_deferred_order(self):
        # Insert execution rows in reverse dependency order inside one transaction, with zero rows
        # in HQ-only connection_turn_permits, exercising both hq_pool and office_local permits.
        sha = "a" * 64
        insert(self.db, "agent_execution_attempts", organization_id="org", id="att-2", turn_id="turn-1",
               attempt_no=2, employee_id="ana", office_id="numbering-b", placement_generation=1,
               connection_id="conn-local", permit_authority="office_local", permit_id="local-permit-1",
               runtime_driver="claude", driver_version="1.0", requested_model_id="m1", resolved_model_id="m1",
               connection_revision=1, binding_revision=1, state="succeeded", effect_state="none",
               predecessor_attempt_id="att-1")
        insert(self.db, "agent_execution_attempts", organization_id="org", id="att-1", turn_id="turn-1",
               attempt_no=1, employee_id="ana", office_id="numbering-b", placement_generation=1,
               connection_id="conn-pool", permit_authority="hq_pool", permit_id="hq-permit-absent-on-satellite",
               runtime_driver="claude", driver_version="1.0", requested_model_id="m1", resolved_model_id="m1",
               connection_revision=1, binding_revision=1, state="failed", effect_state="none")
        insert(self.db, "agent_execution_turns", organization_id="org", id="turn-1", employee_id="ana",
               office_id="numbering-b", placement_generation=1, revision=1, workspace_key="ws",
               input_ref="turns/turn-1/input.json", input_sha256=sha, agent_config_revision=1,
               model_profile_id="prof-1", profile_revision=1, model_id="m1", pool_snapshot_json="[]",
               status="succeeded", created_at="2026-11-21T00:00:00Z")
        insert(self.db, "agent_execution_configs", organization_id="org", employee_id="ana",
               schema_version=1, revision=1, model_profile_id="prof-1", model_id="m1",
               system_prompt_markdown="Work carefully.")
        insert(self.db, "model_profiles", organization_id="org", id="prof-1", schema_version=1, revision=1,
               key="prof-1", name="Prof 1")
        seed_provider_connection(self.db, identity="sub-pool", connection_id="conn-pool")
        seed_provider_connection(self.db, identity="sub-local", connection_id="conn-local")
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM connection_turn_permits").fetchone()[0], 0)
        self.assertEqual(self.db.execute("PRAGMA foreign_key_check").fetchall(), [])
        self.refused(lambda: insert(
            self.db, "agent_execution_attempts", organization_id="org", id="att-bad", turn_id="turn-1",
            attempt_no=3, employee_id="ana", office_id="numbering-b", placement_generation=1,
            connection_id="conn-local", permit_authority="invalid_authority", permit_id="p",
            runtime_driver="claude", driver_version="1.0", requested_model_id="m1", resolved_model_id="m1",
            connection_revision=1, binding_revision=1, state="selected", effect_state="none"))

    def test_a_process_declares_its_tabs_from_the_generic_views(self):
        self.package()
        tab = dict(organization_id="org", process_id="dss", title="Product")
        insert(self.db, "development_process_tabs", **tab, key="product", position=0, view="documents",
               config_json='{"section": "product", "kinds": ["product-dss"]}')
        insert(self.db, "development_process_tabs", **tab, key="board", position=1, view="work",
               config_json='{"types": ["story"], "layout": "board"}')
        self.db.commit()
        # A view is one of the product's; a fixed tab's key is not available; order is unique.
        self.refused(lambda: insert(self.db, "development_process_tabs", **tab, key="calendar",
                                    position=3, view="calendar"))
        insert(self.db, "development_process_tabs", **tab, key="releases", position=2, view="releases")
        self.db.commit()
        for key in ("overview", "work", "docs", "team", "activity", "conversations"):
            self.refused(lambda: insert(self.db, "development_process_tabs", **tab, key=key,
                                        position=3, view="features"))
        self.refused(lambda: insert(self.db, "development_process_tabs", **tab, key="again",
                                    position=1, view="features"))
        self.refused(lambda: insert(self.db, "development_process_tabs", **tab, key="ninth",
                                    position=8, view="features"))
        self.refused(lambda: insert(self.db, "development_process_tabs", **tab, key="bad-config",
                                    position=3, view="work", config_json='["story"]'))

    def test_a_project_follows_an_existing_package_or_none(self):
        self.assertIsNone(self.db.execute("SELECT development_process_id FROM projects").fetchone()[0])
        self.package()
        self.db.execute("UPDATE projects SET development_process_id='dss'")
        self.db.commit()
        self.refused(lambda: self.db.execute("UPDATE projects SET development_process_id='missing'"))

    def test_an_organization_package_needs_an_owner_to_be_current_and_a_built_in_one_does_not(self):
        self.package()
        self.db.commit()
        self.refused(lambda: self.package("custom", origin="organization"))
        self.package("custom", origin="organization", owner_employee_id="reporter")
        self.package("draft", origin="organization", status="draft")
        self.db.commit()
        self.refused(lambda: self.package("other", key="dss"))

    def test_a_skill_is_bound_only_to_roles_of_its_own_package(self):
        self.package()
        self.package("tdd")
        insert(self.db, "development_process_roles", organization_id="org", process_id="dss",
               key="product-lead", position=0, name="Product lead", seed=1)
        insert(self.db, "development_process_roles", organization_id="org", process_id="tdd",
               key="engineer", position=0, name="Engineer")
        insert(self.db, "development_process_skills", organization_id="org", process_id="dss",
               key="write-product-dss", position=0, title="Write the Product DSS",
               description="Use when the person describes behaviour.")
        insert(self.db, "development_process_skill_roles", organization_id="org", process_id="dss",
               skill_key="write-product-dss", role_key="product-lead")
        self.db.commit()
        self.refused(lambda: insert(self.db, "development_process_skill_roles", organization_id="org",
                                    process_id="dss", skill_key="write-product-dss", role_key="engineer"))

    def test_package_keys_folders_and_descriptions_are_well_formed(self):
        self.package()
        self.db.commit()
        role = dict(organization_id="org", process_id="dss", position=0, name="Lead")
        skill = dict(organization_id="org", process_id="dss", position=0, title="Skill")
        folder = dict(organization_id="org", process_id="dss", position=0, title="Folder")
        insert(self.db, "development_process_folders", **folder, path="technical/diagrams")
        self.db.commit()
        for key in ("-lead", "Lead", "product lead", ""):
            self.refused(lambda: insert(self.db, "development_process_roles", **role, key=key))
            self.refused(lambda: insert(self.db, "development_process_skills", **skill, key=key,
                                        description="Use when needed."))
        self.refused(lambda: insert(self.db, "development_process_skills", **skill, key="empty",
                                    description=""))
        for path in ("technical/diagrams", "/technical", "technical/", "technical//diagrams",
                     "technical/v1.2", "../technical", "Technical"):
            self.refused(lambda: insert(self.db, "development_process_folders", **folder, path=path))

    def test_sections_are_folders_of_one_project(self):
        section = dict(organization_id="org", project_id="project", position=0, title="Product")
        insert(self.db, "documentation_sections", **section, path="product")
        insert(self.db, "documentation_sections", **section, path="product/identity-and-access")
        self.db.commit()
        for path in ("product", "product/", "/product", "product//billing", "product/v1.2",
                     "product/spec.md", ".hidden"):
            self.refused(lambda: insert(self.db, "documentation_sections", **section, path=path))
        self.refused(lambda: insert(self.db, "documentation_sections", **dict(section, project_id="missing"),
                                    path="technical"))

    def test_a_package_cannot_redefine_a_built_in_document_kind(self):
        self.package()
        insert(self.db, "development_process_kinds", organization_id="org", process_id="dss",
               key="product-dss", title="Product DSS", versioned=1)
        for kind in ("brief", "decision", "design", "report", "changelog"):
            self.refused(lambda: insert(self.db, "development_process_kinds", organization_id="org",
                                        process_id="dss", key=kind, title=kind))

    def test_project_wide_documents_live_in_the_tree_and_scoped_ones_do_not(self):
        self.document()
        self.document("decision", kind="decision", path="decisions/use-sqlite.md")
        insert(self.db, "documents", organization_id="org", id="brief", schema_version=1, revision=1,
               project_id="project", kind="brief", title="Brief")
        self.db.commit()
        # A project-wide document needs a path; the brief has none.
        self.refused(lambda: self.document("pathless", path=None))
        self.refused(lambda: insert(self.db, "documents", organization_id="org", id="brief-2",
                                    schema_version=1, revision=1, project_id="project", kind="brief",
                                    title="Brief", path="brief.md"))
        # Two documents never share a path, and a path stays inside the tree.
        self.refused(lambda: self.document("twin", path="product/spec.md"))
        for path in ("../spec.md", "/product/spec.md", "product//spec.md", "product/.hidden.md",
                     "product/spec.txt", "Product/Spec.md"):
            self.refused(lambda: self.document("bad", path=path))

    def test_a_package_kind_is_only_for_tree_documents(self):
        insert(self.db, "features", organization_id="org", id="feature", schema_version=1, revision=1,
               project_id="project", local_slug="sign-in", title="Sign in", status="planned")
        insert(self.db, "releases", organization_id="org", id="release", schema_version=1, revision=1,
               project_id="project", title="Release")
        scoped = dict(organization_id="org", schema_version=1, revision=1, project_id="project",
                      release_id="release", title="Scoped")
        insert(self.db, "documents", **scoped, id="release-report", kind="report")
        self.db.commit()
        # Beside a release only the built-in kinds are allowed, and a kind is a key.
        self.refused(lambda: insert(self.db, "documents", **scoped, id="bad", kind="product-dss"))
        self.refused(lambda: self.document("bad", kind="Product DSS!"))
        self.refused(lambda: insert(self.db, "documents", organization_id="org", id="scoped",
                                    schema_version=1, revision=1, project_id="project",
                                    feature_id="feature", kind="product-dss", title="Scoped"))
        self.refused(lambda: insert(self.db, "documents", organization_id="org", id="scoped",
                                    schema_version=1, revision=1, project_id="project",
                                    feature_id="feature", kind="design", title="Scoped",
                                    path="designs/scoped.md"))

    def test_a_release_or_deployment_has_one_changelog(self):
        common = dict(organization_id="org", schema_version=1, revision=1, project_id="project")
        for release in ("r1", "r2"):
            insert(self.db, "releases", **common, id=release, title=release)
        insert(self.db, "deployments", **common, id="d1", release_id="r1", environment="production",
               target="primary", operation="deploy", status="queued")
        changelog = dict(common, kind="changelog", title="Changes")
        insert(self.db, "documents", **changelog, id="r1-log", release_id="r1")
        insert(self.db, "documents", **changelog, id="r2-log", release_id="r2")
        insert(self.db, "documents", **changelog, id="d1-log", deployment_id="d1")
        insert(self.db, "documents", **dict(changelog, kind="report"), id="r1-report", release_id="r1")
        self.db.commit()
        for scope in (dict(release_id="r1"), dict(deployment_id="d1")):
            with self.subTest(scope=scope):
                assert_refused(self, self.db, "UNIQUE constraint failed: documents",
                               lambda: insert(self.db, "documents", **changelog, id="again", **scope))

    def test_only_an_issued_tree_document_carries_a_version(self):
        issued = dict(status="current", owner_employee_id="reporter", revision=3, version=1,
                      issued_revision=2)
        self.document(**issued)
        self.issue(revision=2)
        self.db.commit()
        # The version a document carries must be one that was issued for it.
        self.refused(lambda: self.document("unissued", **issued))
        self.refused(lambda: self.db.execute("UPDATE documents SET version=2 WHERE id='spec'"))
        for name, fields in (("half", dict(version=1)),
                             ("ahead", dict(issued, revision=2, issued_revision=5)),
                             ("draft", dict(version=1, issued_revision=1))):
            self.issue(name)
            self.refused(lambda: self.document(name, **fields))
        # A document outside the tree is never versioned and has no summary.
        self.issue("brief")
        base = dict(organization_id="org", id="brief", schema_version=1, revision=1,
                    project_id="project", kind="brief", title="Brief")
        self.refused(lambda: insert(self.db, "documents", **base, status="current",
                                    owner_employee_id="reporter", version=1, issued_revision=1))
        self.refused(lambda: insert(self.db, "documents", **base, summary="What the project is."))

    def test_an_issued_version_is_written_once(self):
        self.document()
        self.issue()
        self.db.commit()
        for statement in ("UPDATE document_versions SET body_markdown='changed'",
                          "DELETE FROM document_versions",
                          "INSERT OR REPLACE INTO document_versions SELECT * FROM document_versions"):
            self.refused(lambda: self.db.execute(statement))
        self.assertEqual(self.db.execute("SELECT body_markdown FROM document_versions").fetchone()[0],
                         "## Purpose")

    def test_a_version_is_derived_from_issued_versions_of_other_documents(self):
        self.document()
        self.document("technical", kind="technical-dss", path="technical/authentication.md")
        self.issue()
        self.issue("technical")
        self.db.commit()
        insert(self.db, "document_version_sources", organization_id="org", document_id="technical",
               version=1, source_document_id="spec", source_version=1)
        self.db.commit()
        self.refused(lambda: insert(self.db, "document_version_sources", organization_id="org",
                                    document_id="technical", version=1, source_document_id="spec",
                                    source_version=2))
        self.refused(lambda: insert(self.db, "document_version_sources", organization_id="org",
                                    document_id="technical", version=1,
                                    source_document_id="technical", source_version=1))

    def test_a_version_converges_when_its_tasks_are_finished(self):
        self.document()
        self.issue()
        seed_task(self.db)
        seed_task(self.db, task="second")
        progress = lambda: tuple(self.db.execute(
            "SELECT task_count, done_task_count, is_converged FROM document_version_progress"
        ).fetchone())
        self.assertEqual(progress(), (0, 0, 0))  # no task: not converged
        for task in ("task", "second"):
            insert(self.db, "task_document_versions", organization_id="org", task_id=task,
                   document_id="spec", version=1)
        self.db.execute("UPDATE tasks SET status='done' WHERE id='task'")
        self.assertEqual(progress(), (2, 1, 0))
        self.db.execute("UPDATE tasks SET status='cancelled' WHERE id='second'")
        self.assertEqual(progress(), (2, 2, 1))
        self.db.execute("UPDATE tasks SET status='in_progress' WHERE id='second'")
        self.assertEqual(progress(), (2, 1, 0))  # reopening needs no other write
        self.db.commit()
        self.refused(lambda: insert(self.db, "task_document_versions", organization_id="org",
                                    task_id="task", document_id="spec", version=2))

    def test_code_links_and_media_stay_inside_their_roots(self):
        self.document()
        insert(self.db, "document_code_links", organization_id="org", id="link", document_id="spec",
               repository_id="org-repository", path="services/auth/src/login.rs",
               symbol="verify_password")
        insert(self.db, "documentation_media", organization_id="org", id="diagram", schema_version=1,
               revision=1, project_id="project", path="technical/diagrams/login-sequence.mmd",
               type="diagram", media_type="text/vnd.mermaid", bytes=1840, sha256=self.SHA,
               title="Login sequence")
        self.db.commit()
        for path in ("/etc/passwd", "../other/file.rs", "src/../../file.rs", "src/.."):
            self.refused(lambda: insert(self.db, "document_code_links", organization_id="org",
                                        id="bad", document_id="spec", repository_id="org-repository",
                                        path=path))
        self.refused(lambda: insert(self.db, "document_code_links", organization_id="org", id="bad",
                                    document_id="spec", repository_id="missing", path="src/a.rs"))
        for fields in (dict(path="technical/notes.md"), dict(path="../diagram.png"),
                       dict(bytes=5 * 1024 * 1024 + 1), dict(type="video")):
            values = dict(organization_id="org", id="bad", schema_version=1, revision=1,
                          project_id="project", path="technical/diagram.png", type="image",
                          media_type="image/png", bytes=10, sha256=self.SHA, title="Diagram")
            values.update(fields)
            self.refused(lambda: insert(self.db, "documentation_media", **values))

    def test_hot_path_indexes_and_non_quadratic_views(self):
        def plan_lines(conn, sql, params=()):
            return [row[3] for row in conn.execute(f"EXPLAIN QUERY PLAN {sql}", params).fetchall()]

        # Hot-path shared indexes (TASK-6 / H-13, H-10).
        indexed_queries = [
            (self.db, "tasks_by_feature",
             "SELECT id FROM tasks WHERE organization_id=? AND feature_id=?", ("org", "f1")),
            (self.db, "task_document_versions_by_document",
             "SELECT task_id FROM task_document_versions WHERE organization_id=? AND document_id=? AND version=?",
             ("org", "doc", 1)),
            (self.db, "document_code_links_by_document",
             "SELECT id FROM document_code_links WHERE organization_id=? AND document_id=?", ("org", "doc")),
            (self.db, "document_version_sources_by_source",
             "SELECT document_id FROM document_version_sources WHERE organization_id=? AND source_document_id=? AND source_version=?",
             ("org", "src", 1)),
            (self.db, "hire_requests_by_team_state",
             "SELECT id FROM hire_requests WHERE organization_id=? AND home_team_id=? AND state=?",
             ("org", "payments", "hiring")),
            (self.db, "usage_by_release_time",
             "SELECT id FROM usage_records WHERE organization_id=? AND release_id=? ORDER BY occurred_at",
             ("org", "rel")),
            # TASK-105: an open-scope page of a project's or a team's tasks, in the default order.
            (self.db, "tasks_open_by_project",
             "SELECT t.id, t.title FROM tasks t WHERE t.organization_id=? AND t.project_id=? "
             "AND t.status NOT IN ('done', 'cancelled') "
             "ORDER BY t.rank_order IS NULL, t.rank_order, t.office_code, t.number, t.id LIMIT 50",
             ("org", "project")),
            (self.db, "tasks_open_by_team",
             "SELECT t.id, t.title FROM tasks t WHERE t.organization_id=? AND t.team_id=? "
             "AND t.status NOT IN ('done', 'cancelled') "
             "ORDER BY t.rank_order IS NULL, t.rank_order, t.office_code, t.number, t.id LIMIT 50",
             ("org", "team")),
        ]
        msg_db = database("messaging-schema.sql")
        self.addCleanup(msg_db.close)
        bud_db = database("budgets-schema.sql")
        self.addCleanup(bud_db.close)
        indexed_queries.extend([
            (msg_db, "messages_by_thread",
             "SELECT id FROM messages WHERE organization_id=? AND thread_root_id=? ORDER BY created_at",
             ("org", "root")),
            (bud_db, "active_budget_holds",
             "SELECT budget_id FROM budget_holds WHERE organization_id=? AND budget_id=? AND state <> 'released'",
             ("org", "b1")),
        ])
        for conn, expected_index, sql, params in indexed_queries:
            with self.subTest(index=expected_index):
                joined = "\n".join(plan_lines(conn, sql, params))
                self.assertIn(expected_index, joined)
                if expected_index.startswith("tasks_open_by_"):
                    # The open-task partial indexes hold the default order: no sort step.
                    self.assertNotIn("TEMP B-TREE", joined)

        # ADR-0031 person read on private_notes uses private_notes_by_task.
        private = sqlite3.connect(":memory:")
        self.addCleanup(private.close)
        private.executescript((ROOT / "private-notes-schema.sql").read_text())
        private_plan = "\n".join(plan_lines(
            private,
            "SELECT id, author_id, body_markdown FROM private_notes "
            "WHERE organization_id=? AND task_id=? AND deleted_at IS NULL ORDER BY updated_at",
            ("org", "task"),
        ))
        self.assertIn("private_notes_by_task", private_plan)

        # usage_*_totals views must materialize org_incomplete_streams once (never correlated per group).
        for view in ("usage_task_totals", "usage_agent_totals", "usage_project_totals"):
            with self.subTest(view=view):
                lines = plan_lines(self.db, f"SELECT * FROM {view} WHERE organization_id=?", ("org",))
                joined = "\n".join(lines)
                self.assertIn("MATERIALIZE org_incomplete_streams", joined)
                self.assertEqual(sum(1 for l in lines if "CO-ROUTINE usage_stream_coverage" in l), 1, joined)
                # Ensure usage_stream_coverage is NOT evaluated inside a correlated subquery.
                idx_cov = next(i for i, l in enumerate(lines) if "MATERIALIZE org_incomplete_streams" in l)
                self.assertNotIn("CORRELATED", lines[idx_cov - 1] if idx_cov > 0 else "")

        # release_progress must evaluate milestone_progress at most once (no 3x co-routine scan).
        release_plan = plan_lines(self.db, "SELECT * FROM release_progress WHERE organization_id=?", ("org",))
        milestone_evals = sum(1 for line in release_plan if "CO-ROUTINE milestone_progress" in line)
        self.assertLessEqual(milestone_evals, 1, "\n".join(release_plan))

    def test_task_7_integrity_holes_closed(self):
        # H-08: budgeted task can be tombstoned cleanly while budget remains intact in isolated budgets.db via budget_tasks mirror.
        seed_task(self.db, task="budgeted-task")
        bud_db = database("budgets-schema.sql")
        self.addCleanup(bud_db.close)
        seed_budget_mirrors(bud_db)
        insert(bud_db, "budget_tasks", organization_id="org", kind="task", id="budgeted-task", is_deleted=0)
        insert(bud_db, "budgets", organization_id="org", id="b-task", current_revision=1,
               scope_kind="task", task_id="budgeted-task", period_kind="lifetime",
               time_zone="UTC", time_zone_source="hq", created_at="2026-11-21T00:00:00Z")
        insert(bud_db, "budget_policy_versions", organization_id="org", budget_id="b-task",
               revision=1, state="active", unit="tokens", limit_amount=1000,
               changed_by_id="reporter", changed_at="2026-11-21T00:00:00Z")
        bud_db.commit()
        bud_db.execute("UPDATE budget_tasks SET is_deleted=1 WHERE id='budgeted-task'")
        bud_db.commit()
        assert_refused(self, bud_db, "deleted budget task mirror cannot be undeleted",
                       "UPDATE budget_tasks SET is_deleted=0 WHERE id='budgeted-task'")
        insert(self.db, "tombstones", organization_id="org", kind="task", id="budgeted-task",
               schema_version=1, revision=1, project_id="project", office_code="A", number=99,
               terminal_state="done")
        self.db.execute("UPDATE record_refs SET is_deleted=1 WHERE kind='task' AND id='budgeted-task'")
        self.db.execute("DELETE FROM tasks WHERE id='budgeted-task'")
        self.db.commit()
        self.assertEqual(bud_db.execute("SELECT task_id FROM budgets WHERE id='b-task'").fetchone()[0], "budgeted-task")
        self.assertEqual(self.db.execute("SELECT office_code FROM tombstones WHERE id='budgeted-task'").fetchone()[0], "A")

        # H-11: cross-project affects_feature_id refused on deferred insert, task_details.task_id update,
        # and feature.project_id update.
        insert(self.db, "projects", organization_id="org", id="p2", schema_version=1, revision=1,
               slug="p2", name="P2", owner_team_id="payments")
        insert(self.db, "features", organization_id="org", id="f1", schema_version=1, revision=1,
               project_id="project", local_slug="f1", title="F1", status="planned")
        insert(self.db, "features", organization_id="org", id="f2", schema_version=1, revision=1,
               project_id="p2", local_slug="f2", title="F2", status="planned")
        seed_task(self.db, task="t-p1")
        seed_task(self.db, task="t-p2", project="p2")
        # Deferred insert: task_details inserted before task of wrong project must fail on task insert.
        insert(self.db, "task_details", organization_id="org", task_id="deferred-wrong", affects_feature_id="f1")
        assert_refused(self, self.db, "affected feature belongs to another project", lambda: (
            insert(self.db, "record_refs", organization_id="org", kind="task", id="deferred-wrong", is_deleted=0),
            insert(self.db, "tasks", organization_id="org", id="deferred-wrong", office_code="A", number=103,
                   schema_version=1, revision=1, team_id="payments", project_id="p2", title="Wrong", status="todo"),
        ))
        # Updating task_details.task_id to a task in another project must fail.
        insert(self.db, "task_details", organization_id="org", task_id="t-p1", affects_feature_id="f1")
        self.db.commit()
        assert_refused(self, self.db, "affected feature belongs to another project",
                       "UPDATE task_details SET task_id='t-p2' WHERE task_id='t-p1'")
        # Updating features.project_id while referenced by a project-1 task must fail.
        assert_refused(self, self.db, "affected feature belongs to another project",
                       "UPDATE features SET project_id='p2' WHERE id='f1'")

        # H-12: document_versions record_refs FK, same-project superkey, no-replace (even with recursive_triggers=OFF),
        # and document_version_sources immutability & same-project guard.
        self.refused(lambda: self.issue("unregistered-doc", register_ref=False))
        self.document("doc-p1", project_id="project", path="product/doc-p1.md")
        self.document("doc-p2", project_id="p2", path="product/doc-p2.md")
        self.issue("doc-p1", project_id="project")
        self.issue("doc-p2", project_id="p2")
        self.db.commit()
        # Cross-project version source is refused.
        assert_refused(self, self.db, "document version sources must belong to the same project", lambda: insert(
            self.db, "document_version_sources", organization_id="org",
            document_id="doc-p1", version=1, source_document_id="doc-p2", source_version=1))
        # Cross-project task_document_versions is refused.
        assert_refused(self, self.db, "realizing task and document version must belong to the same project", lambda: insert(
            self.db, "task_document_versions", organization_id="org",
            task_id="t-p2", document_id="doc-p1", version=1))
        # Same-project source works and is immutable (no UPDATE / DELETE).
        self.document("doc-p1-b", project_id="project", path="product/doc-p1-b.md")
        self.issue("doc-p1-b", project_id="project")
        insert(self.db, "document_version_sources", organization_id="org",
               document_id="doc-p1-b", version=1, source_document_id="doc-p1", source_version=1)
        self.db.commit()
        assert_refused(self, self.db, "document version sources are never changed",
                       "UPDATE document_version_sources SET source_version=1")
        assert_refused(self, self.db, "document version sources are never removed",
                       "DELETE FROM document_version_sources")
        # BEFORE INSERT no-replace blocks INSERT OR REPLACE on document_versions even if recursive_triggers=OFF.
        self.db.execute("PRAGMA recursive_triggers = OFF")
        try:
            assert_refused(self, self.db, "an issued document version is never replaced",
                           "INSERT OR REPLACE INTO document_versions SELECT * FROM document_versions WHERE document_id='doc-p1'")
        finally:
            self.db.execute("PRAGMA recursive_triggers = ON")

        # H-14: kind CHECKs on record_refs/tombstones/links, release_tasks same-project,
        # and blocking new operational children on tombstoned tasks.
        for bad_kind in ("", "bug"):
            with self.subTest(bad_kind=bad_kind):
                self.refused(lambda: insert(self.db, "record_refs", organization_id="org", kind=bad_kind, id="x", is_deleted=0))
                self.refused(lambda: insert(self.db, "tombstones", organization_id="org", kind=bad_kind, id="x",
                                            schema_version=1, revision=1, terminal_state="done"))
        self.refused(lambda: insert(self.db, "links", organization_id="org", source_kind="task", source_id="t-p1",
                                    id="l-bad", kind="relates_to", target_kind="bogus", target_id="t-p2"))
        # release_tasks same-project check.
        insert(self.db, "releases", organization_id="org", id="rel-p1", schema_version=1, revision=1,
               project_id="project", title="Rel P1")
        self.db.commit()
        assert_refused(self, self.db, "release task belongs to another project", lambda: insert(
            self.db, "release_tasks", organization_id="org", release_id="rel-p1", project_id="project", task_id="t-p2"))
        # New operational children on a tombstoned task (budgeted-task, is_deleted=1) are rejected.
        assert_refused(self, self.db, "cannot attach operational record to a deleted task", lambda: insert(
            self.db, "comments", organization_id="org", target_kind="task", target_id="budgeted-task",
            id="c1", position=0, author_id="reporter", created_at="2026-11-21T00:00:00Z", body_markdown="Late"))
        assert_refused(self, self.db, "cannot attach operational record to a deleted task", lambda: insert(
            self.db, "checklist_steps", organization_id="org", target_kind="task", target_id="budgeted-task",
            id="s1", position=0, text="Late", required=0, done=0))
        assert_refused(self, self.db, "cannot attach operational record to a deleted task", lambda: insert(
            self.db, "watchers", organization_id="org", target_kind="task", target_id="budgeted-task",
            id="w1", watcher_kind="owner"))
        assert_refused(self, self.db, "cannot attach operational record to a deleted task", lambda: insert(
            self.db, "owner_questions", organization_id="org", target_kind="task", target_id="budgeted-task",
            id="q1", question_comment_id="c1", asked_by_id="reporter", state="open", created_at="2026-11-21T00:00:00Z"))
        assert_refused(self, self.db, "cannot attach operational record to a deleted task", lambda: insert(
            self.db, "waiting_spans", organization_id="org", target_kind="task", target_id="budgeted-task",
            id="ws1", reason="waiting", waiting_on_kind="owner", started_at="2026-11-21T00:00:00Z"))

    def test_task_8_secondary_constraints_and_redundancies(self):
        # M-07: 2-cycles refused in features.replaced_by_feature_id & deployments.rollback_of_id.
        insert(self.db, "features", organization_id="org", id="fa", schema_version=1, revision=1,
               project_id="project", local_slug="fa", title="FA", status="retired")
        insert(self.db, "features", organization_id="org", id="fb", schema_version=1, revision=1,
               project_id="project", local_slug="fb", title="FB", status="live", owner_employee_id="reporter")
        self.db.execute("UPDATE features SET replaced_by_feature_id='fb' WHERE id='fa'")
        self.db.commit()
        assert_refused(self, self.db, "feature replacement cycle is not allowed",
                       "UPDATE features SET replaced_by_feature_id='fa' WHERE id='fb'")

        insert(self.db, "releases", organization_id="org", id="r-dep", schema_version=1, revision=1,
               project_id="project", title="R")
        insert(self.db, "deployments", organization_id="org", id="d1", schema_version=1, revision=1,
               project_id="project", release_id="r-dep", environment="prod", target="primary",
               operation="deploy", status="succeeded", started_at="2026-11-21T00:00:00Z", finished_at="2026-11-21T00:01:00Z")
        insert(self.db, "deployments", organization_id="org", id="d2", schema_version=1, revision=1,
               project_id="project", release_id="r-dep", environment="prod", target="primary",
               operation="rollback", rollback_of_id="d1", status="succeeded", started_at="2026-11-21T00:02:00Z", finished_at="2026-11-21T00:03:00Z")
        self.db.commit()
        assert_refused(self, self.db, "deployment rollback cycle is not allowed",
                       "UPDATE deployments SET operation='rollback', rollback_of_id='d2' WHERE id='d1'")

        # M-07: one_project_brief, documents_key_unique, structure_documents_key_unique.
        insert(self.db, "documents", organization_id="org", id="b1", schema_version=1, revision=1,
               project_id="project", kind="brief", key="proj-brief", title="Brief 1")
        self.db.commit()
        assert_refused(self, self.db, "UNIQUE constraint failed", lambda: insert(
            self.db, "documents", organization_id="org", id="b2", schema_version=1, revision=1,
            project_id="project", kind="brief", title="Brief 2"))
        assert_refused(self, self.db, "UNIQUE constraint failed", lambda: insert(
            self.db, "documents", organization_id="org", id="d-dup-key", schema_version=1, revision=1,
            project_id="project", kind="design", key="proj-brief", path="product/dup.md", title="Dup"))
        insert(self.db, "structure_documents", organization_id="org", id="sd1", key="coding-std",
               schema_version=1, revision=1, kind="standard", title="Std 1")
        self.db.commit()
        assert_refused(self, self.db, "UNIQUE constraint failed", lambda: insert(
            self.db, "structure_documents", organization_id="org", id="sd2", key="coding-std",
            schema_version=1, revision=1, kind="standard", title="Std 2"))
        # Same key at a lower scope (team or project) is allowed to override (doc-34 §3.1/§5).
        insert(self.db, "structure_documents", organization_id="org", id="sd3", key="coding-std",
               schema_version=1, revision=1, kind="standard", title="Team Std 1", team_id="payments")
        insert(self.db, "structure_documents", organization_id="org", id="sd4", key="coding-std",
               schema_version=1, revision=1, kind="standard", title="Project Std 1", project_id="project")
        self.db.commit()
        # Duplicate key within the same team scope is refused.
        assert_refused(self, self.db, "UNIQUE constraint failed", lambda: insert(
            self.db, "structure_documents", organization_id="org", id="sd5", key="coding-std",
            schema_version=1, revision=1, kind="standard", title="Team Std Dup", team_id="payments"))

        # M-07: hire_requests agent-only employee & terminal immutability.
        assert_refused(self, self.db, "hire request employee must be an agent", lambda: insert(
            self.db, "hire_requests", organization_id="org", id="hr-person", schema_version=1, revision=1,
            state="hired", requested_by_employee_id="reporter", home_team_id="payments",
            confirmed_at="2026-11-21T00:00:00Z", employee_id="reporter",
            created_at="2026-11-21T00:00:00Z", updated_at="2026-11-21T00:00:00Z"))
        self.hire("hr-term", state="withdrawn", closed_reason="cancelled")
        self.db.commit()
        assert_refused(self, self.db, "terminal hire request is immutable",
                       "UPDATE hire_requests SET state='drafting' WHERE id='hr-term'")

        # M-07: office_codes immutability/no-delete & counters monotonicity.
        assert_refused(self, self.db, "office codes must be retained", "DELETE FROM office_codes WHERE code='A'")
        insert(self.db, "office_task_counters", organization_id="org", office_code="A", last_number=5, reserved_through=10)
        self.db.commit()
        assert_refused(self, self.db, "office task counter cannot decrease",
                       "UPDATE office_task_counters SET last_number=4 WHERE office_code='A'")

        # M-07: checklist_steps unchecked-with-checker & waiting_spans inverted timestamps rejected.
        seed_task(self.db, task="t-m07")
        self.refused(lambda: insert(self.db, "checklist_steps", organization_id="org", target_kind="task",
                                    target_id="t-m07", id="bad-step", position=0, text="x", required=0, done=0,
                                    checked_by_employee_id="reporter", checked_at="2026-11-21T00:00:00Z"))
        self.refused(lambda: insert(self.db, "waiting_spans", organization_id="org", target_kind="task",
                                    target_id="t-m07", id="bad-wait", reason="x", waiting_on_kind="owner",
                                    started_at="2026-11-21T01:00:00Z", ended_at="2026-11-21T00:00:00Z"))

        # M-08: private-notes-schema.sql guards.
        private = sqlite3.connect(":memory:")
        self.addCleanup(private.close)
        private.executescript((ROOT / "private-notes-schema.sql").read_text())
        insert(private, "private_note_authors", organization_id="org", author_id="ana")
        insert(private, "private_note_tasks", organization_id="org", task_id="task")
        private.commit()
        assert_refused(self, private, "new private note must start active at revision 1", lambda: insert(
            private, "private_notes", organization_id="org", id="n-bad-rev", schema_version=1, revision=2,
            task_id="task", author_id="ana", body_markdown="bad",
            created_at="2026-11-21T00:00:00Z", updated_at="2026-11-21T00:00:00Z"))
        assert_refused(self, private, "new private note must start active at revision 1", lambda: insert(
            private, "private_notes", organization_id="org", id="n-bad-del", schema_version=1, revision=1,
            task_id="task", author_id="ana", body_markdown=None,
            created_at="2026-11-21T00:00:00Z", updated_at="2026-11-21T00:00:00Z", deleted_at="2026-11-21T00:00:00Z"))
        insert(private, "private_notes", organization_id="org", id="n1", schema_version=1, revision=1,
               task_id="task", author_id="ana", body_markdown="good",
               created_at="2026-11-21T00:00:00Z", updated_at="2026-11-21T00:00:00Z")
        insert(private, "private_note_command_receipts", organization_id="org", author_id="ana",
               operation_id="op1", request_sha256="a"*64, command="publish", note_id="n1",
               result_revision=1, state="completed", shared_comment_id="c-pub-1",
               created_at="2026-11-21T00:00:00Z", finished_at="2026-11-21T00:00:01Z")
        private.commit()
        # Duplicate shared_comment_id refused.
        assert_refused(self, private, "UNIQUE constraint failed", lambda: insert(
            private, "private_note_command_receipts", organization_id="org", author_id="ana",
            operation_id="op2", request_sha256="b"*64, command="publish", note_id="n1",
            result_revision=1, state="completed", shared_comment_id="c-pub-1",
            created_at="2026-11-21T00:00:02Z", finished_at="2026-11-21T00:00:03Z"))
        # Monotonic is_deleted: 1 -> 0 refused.
        private.execute("UPDATE private_note_tasks SET is_deleted=1 WHERE task_id='task'")
        private.commit()
        assert_refused(self, private, "deleted private note task mirror cannot be undeleted",
                       "UPDATE private_note_tasks SET is_deleted=0 WHERE task_id='task'")

        # M-09: task_details omits branch & decline_reason; task_workspaces allows two agents on same task+repo.
        td_cols = [r[1] for r in self.db.execute("PRAGMA table_xinfo(task_details)")]
        self.assertNotIn("branch", td_cols)
        self.assertNotIn("decline_reason", td_cols)
        insert(self.db, "task_repositories", organization_id="org", task_id="t-m07", repository_id="org-repository", branch="feat/m09")
        insert(self.db, "task_workspaces", organization_id="org", office_id="numbering-a", employee_id="ana",
               task_id="t-m07", repository_id="org-repository", worktree_path="/tmp/wt-ana")
        insert(self.db, "task_workspaces", organization_id="org", office_id="numbering-a", employee_id="reporter",
               task_id="t-m07", repository_id="org-repository", worktree_path="/tmp/wt-reporter")
        self.db.commit()
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM task_workspaces WHERE task_id='t-m07'").fetchone()[0], 2)

        # L-03: aligned slug/handle/kind/sha CHECKs and dev-process position uniqueness.
        self.refused(lambda: insert(self.db, "employees", organization_id="org", id="e-here", handle="here",
                                    schema_version=1, revision=1, kind="person", name="Here", home_team_id="payments"))
        self.refused(lambda: insert(self.db, "documents", organization_id="org", id="d-dash", schema_version=1,
                                    revision=1, project_id="project", kind="-bad", path="product/bad.md", title="Bad"))
        self.refused(lambda: insert(self.db, "release_repository_commits", organization_id="org",
                                    release_id="r-dep", project_id="project", repository_id="org-repository", commit_sha=""))
        self.package("pos-test")
        insert(self.db, "development_process_roles", organization_id="org", process_id="pos-test", key="r1", position=0, name="R1")
        self.db.commit()
        self.refused(lambda: insert(self.db, "development_process_roles", organization_id="org", process_id="pos-test", key="r2", position=0, name="R2"))

    def test_eng4_contracts_state_guarantees_without_sqlite_statements(self):
        """ADR-0018 ENG-4: Documents 06 and 11 and numbering/journal ADRs state guarantees without SQLite statements."""
        decisions_root = _require_decisions_root()
        targets = [org_data_doc(6), org_data_doc(11)]
        for prefix in ("decision-6 ", "decision-19 ", "decision-21 "):
            matches = sorted(decisions_root.glob(f"{prefix}*.md"))
            self.assertEqual(len(matches), 1, f"expected 1 ADR matching {prefix}")
            targets.append(matches[0])
        for path in targets:
            text = path.read_text(encoding="utf-8")
            prose = re.sub(r"```sql\n.*?\n```", "", text, flags=re.S)
            with self.subTest(document=path.name):
                self.assertNotRegex(prose, r"\bBEGIN\s+IMMEDIATE\b",
                                    f"{path.name} prose must not prescribe SQLite BEGIN IMMEDIATE (ENG-4)")
                self.assertNotRegex(prose, r"\bPRAGMA\s+\w+",
                                    f"{path.name} prose must not prescribe SQLite PRAGMA statements (ENG-4)")
        # ADR-0014 Decision 8 defines the engine-neutral acknowledgement class contract.
        adr14 = sorted(decisions_root.glob("decision-14 *.md"))
        self.assertEqual(len(adr14), 1)
        adr14_text = adr14[0].read_text(encoding="utf-8")
        self.assertNotRegex(adr14_text, r"\bBEGIN\s+IMMEDIATE\b")
        self.assertNotRegex(adr14_text, r"\bPRAGMA\s+\w+")

    def test_eng5_schema_coverage_lists_all_triggers_and_generated_columns(self):
        """ADR-0018 ENG-5: Schema coverage (doc-74) lists every trigger and generated column via live SQLite catalog."""
        coverage = schema_coverage_doc().read_text(encoding="utf-8")
        for sql_file in sorted(ROOT.glob("*.sql")):
            conn = database(sql_file.name)
            self.addCleanup(conn.close)
            triggers = [
                row[0]
                for row in conn.execute("SELECT name FROM sqlite_schema WHERE type = 'trigger' ORDER BY name")
            ]
            for trg in triggers:
                with self.subTest(schema=sql_file.name, trigger=trg):
                    self.assertIn(f"`{trg}`", coverage,
                                  f"ENG-5: trigger `{trg}` from {sql_file.name} missing in doc-74 schema coverage")
            tables = [
                row[0]
                for row in conn.execute("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")
            ]
            for tbl in tables:
                for col in conn.execute(f'PRAGMA table_xinfo("{tbl}")'):
                    # hidden=2 (VIRTUAL generated) or hidden=3 (STORED generated)
                    if col[6] in (2, 3):
                        col_ref = f"`{tbl}.{col[1]}`"
                        with self.subTest(schema=sql_file.name, generated=col_ref):
                            self.assertIn(col_ref, coverage,
                                          f"ENG-5: generated column {col_ref} from {sql_file.name} missing in doc-74 schema coverage")

    def test_universal_reviews_strict_tables_triggers_and_indexes(self):
        seed_task(self.db)
        expected_tables = (
            "office_review_counters",
            "reviews",
            "review_rounds",
            "review_reviewers",
            "review_findings",
            "review_finding_replies",
            "review_events",
        )
        for tbl in expected_tables:
            row = self.db.execute(
                "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?", (tbl,)
            ).fetchone()
            self.assertIsNotNone(row, f"missing table {tbl}")
            self.assertTrue(row[0].rstrip().endswith("STRICT"), f"{tbl} must be STRICT")

        insert(self.db, "office_review_counters", organization_id="org",
               office_id="numbering-a", office_code="A", next_number=2)
        oid1 = "a" * 40
        oid2 = "b" * 40
        sha = "c" * 64
        insert(self.db, "reviews", organization_id="org", id="rev-1",
               office_id="numbering-a", office_code="A", number=1,
               schema_version=1, revision=1, project_id="project", task_id="task",
               target_kind="code", repository_id="org-repository", branch="task/T-A42",
               kind_slug="code-review", category_slug="security",
               title="Review login fix", body_markdown="Please review.",
               independent=1, status="open", current_round=1, session_mode="fresh",
               guidance_refs_json='["doc-35"]', criteria_json='["No secrets"]',
               created_by="reporter",
               created_at="2026-11-27T10:00:00.000Z", updated_at="2026-11-27T10:00:00.000Z")
        insert(self.db, "review_rounds", organization_id="org", review_id="rev-1",
               round_number=1, base_oid=oid1, head_oid=oid2, session_mode="fresh",
               submitted_by="reporter", submitted_at="2026-11-27T10:00:00.000Z", note_markdown="")
        insert(self.db, "review_reviewers", organization_id="org", review_id="rev-1",
               round_number=1, reviewer_id="ana", routing="explicit",
               session_mode="fresh", assigned_by="reporter",
               assigned_at="2026-11-27T10:00:00.000Z", removed=0)
        insert(self.db, "review_findings", organization_id="org", id="find-1",
               review_id="rev-1", number=1, round_number=1, author_id="ana",
               severity="blocker", status="open", anchor_kind="file_line",
               anchor_json=canonical({
                   "kind": "file_line", "path": "src/auth.rs",
                   "start_line": 10, "end_line": 12, "anchor_snippet_sha256": sha,
               }),
               body_markdown="Token logged.", attachment_ids_json="[]", revision=1,
               created_at="2026-11-27T10:05:00.000Z", updated_at="2026-11-27T10:05:00.000Z")
        insert(self.db, "review_finding_replies", organization_id="org", id="rep-1",
               finding_id="find-1", review_id="rev-1", round_number=1, author_id="reporter",
               body_markdown="Fixed.", set_status="resolved", attachment_ids_json="[]",
               created_at="2026-11-27T10:10:00.000Z")
        insert(self.db, "review_events", organization_id="org", review_id="rev-1",
               revision=1, event_type="review_requested", actor_id="reporter",
               operation_id="op-1", payload_json='{"round":1}', payload_sha256=sha,
               created_at="2026-11-27T10:00:00.000Z")
        self.db.commit()

        # Generated display key RV-<OFFICE_CODE><N>.
        key = self.db.execute("SELECT key FROM reviews WHERE id='rev-1'").fetchone()[0]
        self.assertEqual(key, "RV-A1")

        # Monotonic counter cannot decrease.
        assert_refused(self, self.db, "office review counter cannot decrease",
                       "UPDATE office_review_counters SET next_number=1 WHERE office_code='A'")

        # Independent review refuses assigning the author as reviewer.
        assert_refused(self, self.db, "independent review forbids assigning the author as reviewer",
                       lambda: insert(self.db, "review_reviewers", organization_id="org",
                                      review_id="rev-1", round_number=1, reviewer_id="reporter",
                                      routing="explicit", session_mode="fresh",
                                      assigned_by="reporter", assigned_at="2026-11-27T10:01:00.000Z",
                                      removed=0))

        # Immutability triggers on review_rounds, review_finding_replies, and review_events
        # (including INSERT OR REPLACE with recursive_triggers ON and OFF).
        for table, update_sql, msg in (
            ("review_rounds", "UPDATE review_rounds SET note_markdown='tampered'", "review rounds are immutable"),
            ("review_finding_replies", "UPDATE review_finding_replies SET body_markdown='tampered'", "review finding replies are immutable"),
            ("review_events", "UPDATE review_events SET event_type='tampered'", "review events are immutable"),
        ):
            assert_refused(self, self.db, msg, update_sql)
            assert_refused(self, self.db, "must be retained", f"DELETE FROM {table}")
            assert_refused(self, self.db, msg, f"INSERT OR REPLACE INTO {table} SELECT * FROM {table}")
            self.db.execute("PRAGMA recursive_triggers = OFF")
            try:
                assert_refused(self, self.db, msg, f"INSERT OR REPLACE INTO {table} SELECT * FROM {table}")
            finally:
                self.db.execute("PRAGMA recursive_triggers = ON")

        # Anchor kind mismatch in review_findings is rejected.
        assert_refused(self, self.db, "CHECK constraint failed", lambda: insert(
            self.db, "review_findings", organization_id="org", id="find-bad",
            review_id="rev-1", number=2, round_number=1, author_id="ana",
            severity="minor", status="open", anchor_kind="file_line",
            anchor_json='{"kind":"general"}', body_markdown="Mismatch.",
            attachment_ids_json="[]", revision=1,
            created_at="2026-11-27T10:06:00.000Z", updated_at="2026-11-27T10:06:00.000Z"))

        # Empty reply without set_status is rejected.
        assert_refused(self, self.db, "CHECK constraint failed", lambda: insert(
            self.db, "review_finding_replies", organization_id="org", id="rep-bad",
            finding_id="find-1", review_id="rev-1", round_number=1, author_id="reporter",
            body_markdown="", set_status=None, attachment_ids_json="[]",
            created_at="2026-11-27T10:11:00.000Z"))

        # Hot-path review indexes resolve cleanly in EXPLAIN QUERY PLAN.
        for idx, sql, params in (
            ("reviews_by_project_status",
             "SELECT id FROM reviews WHERE organization_id=? AND project_id=? AND status=? ORDER BY updated_at DESC",
             ("org", "project", "open")),
            ("reviews_by_task",
             "SELECT id FROM reviews WHERE organization_id=? AND task_id=?",
             ("org", "task")),
            ("review_reviewers_by_reviewer",
             "SELECT review_id FROM review_reviewers WHERE organization_id=? AND reviewer_id=? AND removed=0",
             ("org", "ana")),
            ("review_findings_by_review_status",
             "SELECT id FROM review_findings WHERE organization_id=? AND review_id=? AND status='open'",
             ("org", "rev-1")),
            ("review_finding_replies_by_finding",
             "SELECT id FROM review_finding_replies WHERE organization_id=? AND finding_id=? ORDER BY created_at",
             ("org", "find-1")),
        ):
            plan = "\n".join(r[3] for r in self.db.execute(f"EXPLAIN QUERY PLAN {sql}", params).fetchall())
            self.assertIn(idx, plan, f"{idx} not used: {plan}")
        self.assertEqual(self.db.execute("PRAGMA foreign_key_check").fetchall(), [])


if __name__ == "__main__":
    unittest.main(verbosity=2)
