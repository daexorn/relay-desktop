import importlib.util
import json
from multiprocessing import Pool
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SOURCE = Path(__file__).parents[1] / "bridge/board.py"
spec = importlib.util.spec_from_file_location("board", SOURCE)
board = importlib.util.module_from_spec(spec)
spec.loader.exec_module(board)


def _add(args):
    folder, n = args
    board.Board("t", folder).apply({"op": "create", "title": f"task {n}"}, "agent", "host", "cli")


class BoardTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.board = board.Board("t", self.dir.name)

    def tearDown(self):
        self.dir.cleanup()

    def test_create_move_done_records_who_and_where(self):
        r = self.board.apply([{"op": "category", "id": "smith", "name": "Кузнец", "color": "#d97757"},
                              {"op": "create", "title": "Ковка", "category": "smith", "estimate": 3}], "champ", "pc-1", "relay")
        ident = r["ids"][1]
        self.board.apply({"op": "claim", "id": ident}, "Claude · champ", "dev", "agent")
        self.board.apply({"op": "done", "id": ident, "summary": "0.7.299"}, "Claude · champ", "dev", "agent")
        task = self.board.state()["tasks"][0]
        self.assertEqual((task["status"], task["assignee"], task["done_by"]), ("done", "Claude · champ", "Claude · champ"))
        self.assertEqual(task["comments"][-1]["text"], "0.7.299")
        log = self.board.activity()
        self.assertEqual(log[0]["host"], "dev")
        self.assertIn("В работе → Готово", log[0]["text"])
        self.assertEqual(self.board.state()["progress"]["categories"]["smith"]["percent"], 100.0)

    def test_progress_weights_estimates_and_partial_work(self):
        self.board.apply([{"op": "create", "title": "a", "estimate": 3, "status": "done"},
                          {"op": "create", "title": "b", "estimate": 1, "progress": 50}], "x", "h", "cli")
        p = self.board.state()["progress"]["all"]
        self.assertEqual((p["tasks"], p["done"], p["percent"]), (2, 1, 87.5))

    def test_links_reorder_and_delete_cleanup(self):
        ids = self.board.apply([{"op": "create", "title": t, "status": "todo"} for t in "abc"], "x", "h", "cli")["ids"]
        self.board.apply({"op": "link", "id": ids[0], "to": ids[1], "kind": "blocks"}, "x", "h", "cli")
        self.board.apply({"op": "move", "id": ids[2], "status": "todo", "before": ids[0]}, "x", "h", "cli")
        order = [t["id"] for t in sorted(self.board.state()["tasks"], key=lambda t: t["order"])]
        self.assertEqual(order, [ids[2], ids[0], ids[1]])
        self.board.apply({"op": "delete", "id": ids[1]}, "x", "h", "cli")
        self.assertEqual(self.board.state()["tasks"][0]["links"], [])

    def test_failed_batch_changes_nothing(self):
        self.board.apply({"op": "create", "title": "a"}, "x", "h", "cli")
        with self.assertRaises(ValueError):
            self.board.apply([{"op": "create", "title": "b"}, {"op": "move", "id": "T-9", "status": "done"}], "x", "h", "cli")
        state = self.board.state()
        self.assertEqual((len(state["tasks"]), state["rev"]), (1, 1))

    def test_unchanged_state_is_cheap(self):
        rev = self.board.apply({"op": "create", "title": "a"}, "x", "h", "cli")["rev"]
        self.assertEqual(self.board.state(since=rev), {"rev": rev, "unchanged": True})

    def test_parallel_writers_lose_nothing(self):
        with Pool(8) as pool:
            pool.map(_add, [(self.dir.name, n) for n in range(40)])
        state = self.board.state()
        self.assertEqual(len({t["id"] for t in state["tasks"]}), 40)
        self.assertEqual(len(self.board.activity()), 40)

    def test_categories_create_rename_reorder_and_delete(self):
        made = self.board.apply([{"op": "category", "new": True, "name": "Корабли и море"},
                                 {"op": "category", "new": True, "name": "Кузня", "color": "#AABBCC"}], "champ", "pc", "relay")["ids"]
        self.assertEqual(made[0], "корабли-и-море")
        ships, smith = made
        task = self.board.apply({"op": "create", "title": "Якорь", "category": ships}, "champ", "pc", "relay")["ids"][0]
        self.board.apply({"op": "category", "id": ships, "name": "Флот", "color": "#112233", "description": "всё о судах"}, "champ", "pc", "relay")
        state = self.board.state()
        fleet = next(c for c in state["categories"] if c["id"] == ships)
        self.assertEqual((fleet["name"], fleet["color"], fleet["description"]), ("Флот", "#112233", "всё о судах"))
        self.assertEqual(state["tasks"][0]["category"], ships)
        self.assertIn("«Корабли и море» → «Флот»", state["activity"][0]["text"])
        self.board.apply({"op": "reorder_categories", "ids": [smith, ships]}, "champ", "pc", "relay")
        self.assertEqual([c["id"] for c in self.board.state()["categories"]], [smith, ships])
        self.board.apply({"op": "category", "id": ships, "delete": True, "move_to": smith}, "champ", "pc", "relay")
        state = self.board.state()
        self.assertEqual([c["id"] for c in state["categories"]], [smith])
        self.assertEqual(state["tasks"][0]["category"], smith)
        self.assertIn("задачи (1) → Кузня", state["activity"][0]["text"])
        self.assertEqual(task, "T-1")

    def test_category_validation(self):
        self.board.apply({"op": "category", "new": True, "name": "Кузня"}, "x", "h", "cli")
        for bad in ({"op": "category", "new": True, "name": "кузня"},
                    {"op": "category", "new": True, "name": "  "},
                    {"op": "category", "id": "кузня", "color": "red"},
                    {"op": "category", "id": "кузня", "name": ""},
                    {"op": "category", "id": "кузня", "delete": True, "move_to": "нет-такой"},
                    {"op": "category", "id": "нет-такой", "delete": True}):
            with self.assertRaises(ValueError, msg=bad):
                self.board.apply(bad, "x", "h", "cli")
        self.assertEqual(len(self.board.state()["categories"]), 1)

    def test_agent_category_tool(self):
        text = board.call(self.board, "board_category", {"name": "Экономика", "color": "#e5b84b"}, "Claude", "dev", "agent")
        self.assertIn("экономика", text)
        board.call(self.board, "board_category", {"id": "экономика", "description": "торговля"}, "Claude", "dev", "agent")
        self.assertEqual(self.board.state()["categories"][0]["description"], "торговля")

    def test_mcp_and_cli_share_the_board(self):
        env = dict(os.environ, RELAY_BOARD_DIR=self.dir.name, RELAY_BOARD_NAME="t", RELAY_BOARD_ACTOR="Claude · test")
        subprocess.run([sys.executable, str(SOURCE), "add", "Плавильня", "--estimate", "2"], env=env, check=True, capture_output=True)
        calls = [{"jsonrpc": "2.0", "id": 1, "method": "tools/list"},
                 {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "board_claim", "arguments": {"id": "T-1"}}},
                 {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "board_overview", "arguments": {}}}]
        out = subprocess.run([sys.executable, str(SOURCE), "mcp"], input="".join(json.dumps(c) + "\n" for c in calls),
                             env=env, text=True, capture_output=True, check=True).stdout.splitlines()
        frames = [json.loads(l) for l in out]
        self.assertIn("board_claim", {t["name"] for t in frames[0]["result"]["tools"]})
        self.assertIn("T-1 [doing]", frames[2]["result"]["content"][0]["text"])
        self.assertEqual(self.board.activity()[0]["actor"], "Claude · test")


if __name__ == "__main__":
    unittest.main()
