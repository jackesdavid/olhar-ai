# Skill: Write Spec (Direct)

## When to use this skill

Use this when the ask is **already clear**: the human has a concrete request with a known user, a known outcome, and at least a rough sense of acceptance criteria. This is the fast path — typically a developer drafting a spec for themselves.

If the idea is vague, novel, or stakeholder-driven, use `write_spec_socratic.md` instead.

---

## Procedure

1. **Capture the ask.** Ask the human (or paste from the ticket): Goal, User, Acceptance Criteria, Risks. One message, not five.

2. **Sharpen each field** with this checklist:

   - **Goal** — one sentence, names a user action. Not a feature. Not a tech stack name.
   - **User Value** — one line: who benefits and what changes for them.
   - **Requirements** — 2–5 bullets of in-scope work. No "and also…".
   - **Acceptance Criteria** — 3–6 observable checkboxes. Use concrete verbs (*create, save, return, display, reject, log*) not vague ones (*improve, enhance, support*).
   - **Dependencies** — prior specs or external constraints (or "none").
   - **Risks** — at least one, even if low.
   - **Notes** — explicit out-of-scope items and any deferred ideas.

3. **Pick the next spec id.** Look at `ai/specs/` and use the next free 3-digit number. Choose a short kebab-case name.

4. **Create the folder:** `ai/specs/<NNN>-<name>/`. Copy `ai/specs/_template/spec.md` into it and fill it from your captured ask.

5. **Run the PM gate** (checklist in `ai/skills/validate_handoff.md`):

   - [ ] Goal is one sentence and names a user action
   - [ ] All AC are observable
   - [ ] In-scope and out-of-scope explicit
   - [ ] At least one risk named
   - [ ] Fits on one screen

   If anything fails, fix it before handoff.

6. **Update `ai/STATE.md`:** set `current_spec`, advance phase to `pm`, append a decision line.

7. **Hand off to PM.** In HITL mode, ask the human to confirm before invoking `ai/roles/pm.md`.

---

## When to switch to the Socratic flow

Stop and recommend `/sdd-spec-socratic` if **two or more** of these are true:

- The human can't state the problem in one sentence without naming a solution
- No persona has been named
- Acceptance criteria are absent or use vague verbs
- The ask conflicts with the project vision in a way you can't reconcile

---

## Anti-patterns

- ❌ Writing the spec without showing it to the human for confirmation
- ❌ Accepting "improve X" as an acceptance criterion
- ❌ Letting scope drift inside one spec — capture extras as future specs
- ❌ Skipping the PM gate "because the spec looks obvious"
