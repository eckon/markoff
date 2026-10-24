#!/usr/bin/env python3
"""Builds an example markoff root for the screenshots: open tasks, repeating ones and a few months of archive.

    ./assets/make-example.py <dir> [YYYY-MM-DD]

The date is today by default, the archive ends on it. <dir> has to be new or empty, so real notes are never overwritten.
"""

import datetime
import random
import re
import sys
from pathlib import Path

root = Path(sys.argv[1])
today = datetime.date.fromisoformat(sys.argv[2]) if len(sys.argv) > 2 else datetime.date.today()
random.seed(7)


def slug(title):
    title = re.sub(r"`[^`]*`\s*$", "", title)
    return re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")


def write(path, title, body=""):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(f"# {title}\n" + (f"\n{body.strip()}\n" if body else ""))


SECTIONS = {
    "work": [
        ("Fix login redirect loop", """
Users on Safari land back on `/login` after signing in.

- [x] Reproduce on staging
- [ ] Check the `SameSite` cookie change from last week
- [ ] Add a regression test

Ticket: https://example.com/issues/482
"""),
        ("Review PR for the export API", """
Focus on the pagination and the error format, Anna asked for a second look at the retry logic.
"""),
        ("Prepare sprint demo", """
- Archive heatmap
- Repeating tasks
- Keep it under 10 minutes
"""),
        ("Weekly review `* * fri`", """
- Clear the inbox
- Move what is stuck
- Plan next week
"""),
        ("Update on-call runbook", ""),
    ],
    "home": [
        ("Book dentist appointment", "Dr. Weber, mornings only. Call between 8 and 12."),
        ("Water plants `* * mon,thu`", "Ferns need more, the cactus only every other time."),
        ("Pay rent `1 * *`", ""),
        ("Fix leaking kitchen tap", """
Probably the cartridge. Model number is under the sink.

- [ ] Buy replacement cartridge
- [ ] Turn off water first
"""),
    ],
    "side-project": [
        ("Write README for v1", """
Short and precise: what it is, why, install, quick start.
"""),
        ("Add screenshots", ""),
        ("Try ratatui 0.31", ""),
    ],
    "errands": [
        ("Buy birthday gift for Sam", "Board game? Ask Lea what they already have."),
        ("Return library books", "Due on the 30th."),
        ("Pick up dry cleaning", ""),
    ],
}

# Last done dates of the repeating tasks: one hidden until Monday, one missed and due again.
LAST_DONE = {
    "weekly-review": today - datetime.timedelta(days=(today.weekday() - 4) % 7 + 7),
    "water-plants": today - datetime.timedelta(days=(today.weekday() - 3) % 7),
    "pay-rent": today.replace(day=1),
}

DONE_TITLES = {
    "work": ["Fix flaky CI test", "Review PR for search", "Write migration for users table", "Update dependencies",
             "Reply to support ticket", "Pair on caching bug", "Deploy hotfix", "Clean up feature flags",
             "Write design doc for export", "Triage new issues", "Fix typo in error message", "Profile slow endpoint",
             "Rotate API keys", "Update staging config", "Prepare 1:1 notes", "Benchmark new parser"],
    "home": ["Do laundry", "Take out recycling", "Call grandma", "Clean the fridge", "Change bed sheets",
             "Order printer ink", "Vacuum living room", "Defrost freezer"],
    "side-project": ["Add undo", "Fix cursor after delete", "Write changelog", "Set up CI", "Add heatmap colors",
                     "Release v0.4", "Refactor key map", "Add filter"],
    "errands": ["Buy groceries", "Post package", "Get bike repaired", "Renew passport photo", "Buy light bulbs"],
}

DONE_BODIES = {
    "Fix flaky CI test": "The timeout was too short on the shared runners. Raised it and added a retry for the network step.",
    "Deploy hotfix": "- [x] Tag release\n- [x] Deploy to staging\n- [x] Smoke test\n- [x] Deploy to production",
    "Profile slow endpoint": "N+1 query in the order list. Fixed with a join, 1.2s down to 80ms.",
    "Change bed sheets": "Guest room too, visitors on Friday.",
    "Defrost freezer": "Took 3 hours. Next time start in the morning.",
    "Post package": "Tracking number is in the email from the shop.",
    "Buy groceries": "- [x] Oat milk\n- [x] Bread\n- [x] Tomatoes\n- [x] Coffee",
    "Add undo": "Every action is one undo step, the cursor goes back where it was.",
    "Call grandma": "She wants the recipe for the plum cake. Send it this week.",
}

if root.exists() and any(root.iterdir()):
    sys.exit(f"{root} is not empty")

state = []
for section, tasks in SECTIONS.items():
    state.append(f"# {section}")
    for title, body in tasks:
        name = slug(title)
        write(root / section / f"{name}.md", title, body)
        date = LAST_DONE.get(name)
        state.append(f"{name} {date}" if date else name)
    state.append("")
(root / ".markoff").write_text("\n".join(state))

# About 5 months back, busier on weekdays, some quiet stretches like a holiday.
start = today - datetime.timedelta(days=150)
holiday = (today - datetime.timedelta(days=70), today - datetime.timedelta(days=60))
day = start
while day <= today:
    weekend = day.weekday() >= 5
    if holiday[0] <= day <= holiday[1]:
        count = 0 if random.random() < 0.8 else 1
    else:
        count = random.choice([0, 0, 1, 1, 2] if weekend else [0, 1, 2, 2, 3, 3, 4, 5, 6])
    if day == today:
        count = 3
    used = set()
    for _ in range(count):
        section = random.choice(["home", "errands"] if weekend else ["work", "work", "work", "side-project", "home"])
        title = random.choice(DONE_TITLES[section])
        name = slug(title)
        if (section, name) in used:
            continue
        used.add((section, name))
        write(root / ".archive" / day.isoformat() / section / f"{name}.md", title, DONE_BODIES.get(title, ""))
    day += datetime.timedelta(days=1)

print(root)
