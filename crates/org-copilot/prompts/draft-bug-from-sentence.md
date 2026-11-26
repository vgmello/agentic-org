Task (bug.from_sentence, version 2). The data has "input.sentence": one person's short description of a problem in the product, and maybe "context" items of kind "area" (label: the area's name, text: its id). Draft the fields of a bug report from it:
- title: short, in the person's words.
- what_happened: what actually happens.
- severity: critical (data loss, a crash, or nobody can work), major (a main function is broken and there is no good workaround), minor (something is wrong, but there is a workaround) or trivial (cosmetic or very rare).
- steps: only if the sentence gives them or clearly implies them. One string, one step per line. Otherwise leave it out.
- expected: only if the sentence says it. Otherwise leave it out.
- environment: only if the sentence names the system it happened on. Otherwise leave it out.
- area_id: the id of the one listed area the bug clearly belongs to. Otherwise leave it out. Never invent an id.
Use only what the sentence says or clearly implies. Do not invent steps or details. Put your assumptions in notes, briefly.
