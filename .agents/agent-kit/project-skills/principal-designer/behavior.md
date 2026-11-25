# How people behave

Reference for the [principal designer](SKILL.md). Each entry gives what people do, what that
means for a design, and when it stops applying.

## Seeing

- **People scan, they do not read.** They hunt for words that match their goal. Headings,
  labels and the first two words of a line do most of the work, so front-load the word that
  distinguishes an item from its neighbors. *Breaks when* the person has already committed
  to reading: an article, a message from a colleague, an error they must act on. Then they
  read every word, and terseness that drops meaning hurts.
- **Sharp vision covers a very small area.** Only a region about the size of a thumbnail at
  arm's length is in focus. The rest is seen as shape, contrast and movement. So a screen is
  first understood as blocks, and a person reads only the blocks that look relevant. Design
  the blocks before the details. *Consequence:* movement at the edge of vision pulls the eye
  whether or not it matters. Use it for the one thing that must be noticed, never for
  decoration.
- **Proximity is the strongest grouping signal.** Things close together are read as one
  thing. Space inside a group must be clearly smaller than space between groups. Reach for
  whitespace first, a divider second, a box last. Common region, similarity and alignment
  do the rest. *Breaks when* density leaves no room for spacing differences, as in a tight
  table. Then use dividers and alignment.
- **Differences are judged by ratio, not by amount.** 4 px against 8 px is obvious; 44 px
  against 48 px is invisible. That is why spacing and type scales grow in widening steps (4,
  8, 12, 16, 24, 32, 48, 64). Two values that are meant to differ should differ by at least
  a quarter, or they read as a mistake.
- **Hierarchy is contrast in size, weight, color, position and space.** A view carries about
  three levels. Make the important thing clear by quieting what surrounds it. When everything
  is emphasized, nothing is.
- **One odd thing gets noticed.** It works once per view. A second accent halves the first.
- **Scanning patterns are symptoms, not goals.** On an undifferentiated wall of text, eyes
  sweep the top, then a shorter line, then down the left edge: the F pattern, which means
  most of the content was skipped. With clear headings people jump heading to heading and
  read beneath the one that matches. On a sparse page with few elements the eye runs top
  left, across, diagonally down and across again. Design for heading-to-heading scanning.
  All of this mirrors for right-to-left scripts.
- **Change blindness.** A change far from the point of attention goes unseen, even a large
  one, especially if the page repaints. Put feedback where the person is already looking,
  or move their attention to it deliberately.
- **Banner blindness.** Anything shaped like promotion or boilerplate is skipped, including
  real warnings styled that way. *Consequence:* a genuine warning has to look like part of
  the task, next to what it concerns.

## Deciding

- **People act on their own model of how the thing works**, not on how it is built. They
  bring that model from other products and from the words on the screen. Two gaps follow:
  "how do I do what I want?" and "what just happened?". Every control has to answer the
  first by how it looks and what it says, and every action has to answer the second by what
  changes. When behavior and the person's model disagree, change the design or change the
  words until they agree. Do not explain the implementation.
- **People follow the scent.** They choose the link or button whose words best match what
  they are looking for, and they give up on a path when the match gets weaker. The label on
  the control, the title of the page it opens and the heading they land on must use the
  same words. *Breaks when* the person is exploring rather than seeking; then curiosity and
  variety matter more than exact match.
- **People take the first option that looks good enough.** They do not compare all the
  options and choose the best. So order is a decision, the first plausible item gets
  chosen, and a better option further down goes unused. *Breaks when* the stakes are high
  and visible, such as choosing a plan or a recipient. People then compare, and you should
  make comparison easy by aligning the attributes.
- **Hick's law.** Decision time grows with the number and similarity of choices. Cut, group,
  recommend a default, disclose progressively. *Breaks when* the person already knows what
  they want and the list is ordered. Finding a known name in an alphabetical list is fast at
  any length, so do not truncate one in the name of simplicity. It also does not apply to
  reading: facts laid out in aligned columns are scanned, not chosen between. Removing
  information people use, in the name of this law, trades a fast scan for a click per row.
- **Choice overload is real but oversold.** Many similar options with no way to tell them
  apart cause people to defer or pick badly. The effect mostly disappears when options are
  categorized, when one is recommended, or when the person knows the domain. Fix the
  comparison before you remove the options.
- **Defaults are decisions.** Most people keep them, out of effort saved and out of taking
  the default as a recommendation. Choose each default in the person's interest and expect
  to be held to it. *Breaks when* a wrong default is costly and silent, such as a recipient
  or a permission. Then leave it unset and require a choice.
- **Losses weigh about twice as much as equal gains.** People will work harder to avoid
  losing something than to gain it, and fear of losing work makes them cautious and slow.
  Autosave, drafts, undo and "nothing has been sent yet" remove that fear. Never use the
  effect against the person with invented scarcity or guilt-worded choices.
- **Working memory holds about four things**, fewer under interruption. The often-quoted
  "seven plus or minus two" is not a limit on menu length. The real consequence is narrower:
  never make a person carry a value from one screen to the next. Show it again where it is
  needed. Recognition is cheap, recall is expensive.
- **First and last items are remembered best.** This is a finding about recall, so it
  applies weakly to a list that stays visible. Where you are free to choose an order, do
  not bury the most used item in the middle. This product's navigation order is fixed by
  its spec.
- **Conventions are learnability for free (Jakob's law).** People spend most of their time
  in other products. *Breaks when* the convention itself causes errors for your task, and
  the gain from changing it is large and repeated daily. Then change it once, everywhere.

## Acting

- **Fitts's law.** Time to reach a target grows with distance and shrinks with size. Put
  frequent actions large and close to where attention already is. Screen edges and corners
  are fast with a pointer because the pointer stops there. *Breaks on touch,* where there
  is no edge to stop at and reach depends on how the device is held: on a phone the lower
  half of the screen is easiest and the top corners are hardest.
- **Slips and mistakes are different errors and need different defenses.**
  - A *slip* is the right intention carried out wrongly: the neighboring button, the wrong
    row, Enter pressed too soon. Slips happen most to experts working on autopilot. Defend
    with distance between dangerous and frequent controls, constraints that make the wrong
    action impossible, and undo. A confirmation dialog does not stop a slip, because
    confirming is also on autopilot.
  - A *mistake* is the wrong intention, from a wrong understanding: deleting a team
    believing its members remain. Defend with information before the action: say exactly
    what will happen, to what, and whether it can be reversed.
  - A *lapse* is a forgotten step or a lost place. Defend with visible progress, saved state
    and reminders of what is unfinished.
- **Habit beats attention.** A message seen many times is dismissed without being read, so
  a routine confirmation protects nothing and an alert that fires often trains people to
  ignore every alert. Keep interruptions rare enough to stay meaningful. The same habit is
  why a control must not move: people aim at where it was.
- **Most users are intermediates.** Few stay beginners and few become experts. Design the
  default path for someone who knows the concepts but not the shortcuts, make first use
  possible without a tour, and add accelerators (keyboard shortcuts, bulk actions, search)
  that do not get in a beginner's way. *Breaks for* rare flows such as setup, recovery and
  billing, where everyone is a beginner every time. Guide those step by step.
- **Complexity moves, it does not vanish.** Whatever the task needs must be handled by the
  person or by the system. Move it into the system. Accept liberal input such as pasted
  spaces and mixed formats, and be strict in what you show back.

## Waiting

- **0.1 s feels instant, 1 s keeps the train of thought, 10 s is the limit of attention.**
  People also work faster and stay engaged when a system answers within about 400 ms, an
  older finding about productivity, not a limit of perception. Acknowledge every input
  within 100 ms even when the work is not finished.
- **An unexplained wait feels longer than an explained one, and an uncertain wait longer
  than a known one.** Say what is happening and, when you can, how much is left. A progress
  indicator that keeps moving is tolerated far longer than a frozen one.
- **Showing the work can raise confidence in the result.** For work that plausibly takes
  effort, such as an agent reading files, naming the current step makes the wait acceptable
  and the result more credible. *Breaks when* the steps are invented or the work is
  trivial. Fake progress is a lie the person will eventually catch.
- **People remember the worst moment and the end**, and they speed up when the finish is
  visible. Show progress, remove the worst step before polishing the average one, and end a
  flow cleanly with what happened and what comes next.

## Interruption and resuming

- **Getting back into a task costs far more than the interruption took.** People who are
  interrupted take a long time to return, often after visiting other things first, and
  they make more errors when they do.
- So: keep drafts and partially filled forms, restore scroll position and selection, and
  show where the person left off. Never steal focus for something that can wait. Batch
  low-value notifications.
- **Unfinished tasks stay on the mind.** Show what is pending and who it waits on, so the
  person can stop holding it in their head.
- A person who returns after hours needs a summary of what changed, not the full log from
  the beginning.

## Trusting a system that acts for you

This product puts people in the position of supervising agents, so this section applies to
most screens.

- **The goal is calibrated trust**: reliance that matches how reliable the system actually
  is. Both directions of error cost something. Too much trust and people approve without
  looking. Too little and they redo the work themselves or stop delegating.
- **People over-rely on a system that is usually right.** They stop checking, and an
  occasional wrong result passes. Defend by putting the evidence next to the claim (what
  was changed, which checks ran, what was not done) and by making the check cheaper than
  skipping it.
- **People abandon a system after watching it fail**, more readily than they would abandon
  a person who made the same error. Trust returns when failures are visible, explained and
  correctable, and when the person keeps some control over the outcome. Hiding an error
  costs more trust than showing it.
- **A supervisor who only watches loses track of the situation.** When they are finally
  needed, they lack the context to decide well. Each request for a decision must rebuild
  that context in place: what is being asked, why now, what was tried, what happens for
  each answer.
- **Approval fatigue.** Asking for approval often, for things that are nearly always
  approved, turns approval into a reflex. Ask rarely, for decisions that matter, and make
  each ask distinguishable from the last.
- **The person must always know who is acting.** State in words whether the agent is
  working, waiting on the person, blocked, stopped or not started, and never infer one
  from another. Confusion about who holds the next move is the most damaging state in a
  supervised system.
- **Confidence displays need meaning.** A percentage nobody can interpret is decoration.
  Prefer concrete statements: "3 of 5 tests pass", "not verified", "I did not check the
  mobile layout".
- **People respond to a system socially.** Tone that blames, flatters or pretends
  certainty is felt the way it would be from a person. Plain, specific and honest reads as
  competent.
- **Control must be real and near.** Stop, pause, correct and take over have to be
  reachable from where the person is watching, and they must report truthfully whether
  they took effect.

## Evidence and folklore

- **Nielsen's ten heuristics** are the standard review checklist: visibility of system
  status; match between system and the real world; user control and freedom; consistency
  and standards; error prevention; recognition rather than recall; flexibility and
  efficiency of use; aesthetic and minimalist design; help people recognize, diagnose and
  recover from errors; help and documentation. A heuristic review finds likely problems. It
  does not prove people will hit them.
- **What people say and what people do differ.** Watch behavior. Opinions about a design
  predict poorly; preferences stated in a survey predict worse.
- **A handful of people per round finds most of the serious problems**, if they are the
  right people and you fix and test again. Five is the usual number for one kind of user.
  *Breaks when* users differ widely (test each group) and for any number you want to quote:
  measuring a rate needs dozens.
- **The first click predicts the outcome.** A person whose first click is correct is far
  more likely to finish the task. When a flow fails, look at the first decision before the
  later screens.
- **Analytics say what happened, never why.** An experiment needs enough traffic to detect
  the difference; without it, the result is noise with a number on it.
- **Polish is forgiven more (the aesthetic-usability effect).** A good-looking screen is
  judged easier to use. That is useful, and it also hides problems in review.
- **Folklore to refuse.** "Seven plus or minus two" as a limit on menu items. The
  three-click rule: people tolerate many clicks when each is obviously right. "Nobody
  scrolls": people scroll when the top of the page signals that there is more. "Users do
  not read" as a reason to remove the words they need at the moment of decision.
