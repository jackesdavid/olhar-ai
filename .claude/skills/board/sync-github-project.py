#!/usr/bin/env python3
"""Full resync: docs/Task Board.md (Obsidian Kanban) → GitHub Project #3 "OAI Board".

The Obsidian board is the SOURCE OF TRUTH; this mirrors it to a shared GitHub Project the
co-founder can see. It WIPES the Project's items and recreates one draft per card with the
mapped Status — so any manual edit made directly in the GitHub Project is lost on the next run.

Run:  gh auth switch --user jackesdavid   # needs the `project` scope + repo access
      python3 .claude/skills/board/sync-github-project.py

For a SINGLE card move, prefer the targeted `gh project item-edit` in SKILL.md (§GitHub Project
mirror) instead of a full resync.
"""
import json
import os
import re
import subprocess
import sys

OWNER = "jackesdavid"
PROJ_NUM = "3"
PROJECT_ID = "PVT_kwHOAE3EiM4BeByU"
STATUS_FIELD = "PVTSSF_lAHOAE3EiM4BeByUzhYe_68"
# Obsidian column -> (GitHub Status single-select option id, title prefix)
COLMAP = {
    "Backlog":     ("f75ad846", ""),      # Backlog
    "ToDo":        ("61e4505c", ""),      # Ready
    "In Progress": ("47fc9ee4", ""),      # In progress
    "Review":      ("df73e18b", ""),      # In review
    "Done":        ("98236657", ""),      # Done
    "Bugs":        ("61e4505c", "🐛 "),   # bugs live in Ready, flagged
}
# repo-root-relative; falls back to the absolute path on this machine.
BOARD = os.path.join(os.path.dirname(__file__), "..", "..", "..", "docs", "Task Board.md")
if not os.path.exists(BOARD):
    BOARD = "/Users/jackesdavid/projects/jacktech/bishop/docs/Task Board.md"


def gh(*args):
    r = subprocess.run(["gh", *args], capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args)} failed: {r.stderr.strip()}")
    return r.stdout.strip()


def parse_board():
    """-> list of (column, title) in board order (columns present in COLMAP only)."""
    cards, col = [], None
    card_re = re.compile(r"^- \[[ xX]\]\s+(.*\S)\s*$")
    with open(BOARD, encoding="utf-8") as f:
        for line in f:
            m = re.match(r"^## (.+?)\s*$", line)
            if m:
                col = m.group(1).strip()
                continue
            cm = card_re.match(line)
            if cm and col in COLMAP:
                cards.append((col, cm.group(1).strip()))
    return cards


def main():
    items = json.loads(gh("project", "item-list", PROJ_NUM, "--owner", OWNER,
                          "--format", "json", "--limit", "500"))["items"]
    print(f"Wiping {len(items)} existing item(s)…")
    for it in items:
        gh("project", "item-delete", PROJ_NUM, "--owner", OWNER, "--id", it["id"])

    cards = parse_board()
    print(f"Creating {len(cards)} card(s)…")
    counts = {}
    for i, (col, title) in enumerate(cards, 1):
        opt, prefix = COLMAP[col]
        created = json.loads(gh("project", "item-create", PROJ_NUM, "--owner", OWNER,
                                "--title", prefix + title, "--format", "json"))
        gh("project", "item-edit", "--id", created["id"], "--project-id", PROJECT_ID,
           "--field-id", STATUS_FIELD, "--single-select-option-id", opt)
        counts[col] = counts.get(col, 0) + 1
        if i % 10 == 0 or i == len(cards):
            print(f"  {i}/{len(cards)}")
    print("Done. Per-column:", counts)


if __name__ == "__main__":
    sys.exit(main())
