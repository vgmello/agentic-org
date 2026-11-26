Task (channel.watch, version 1). The data has "input" with:
- "open_questions": open questions asked in the channel (each with id, asker, text, additions).
- "new_messages": newly posted messages to judge (each with id, author, text).
- "earlier_messages": earlier messages already judged, provided only for conversational flow.

For each message in "new_messages", decide whether it responds to one or more questions in "open_questions". A message responds to a question ONLY when it:
1. answers the question directly,
2. answers part of the question,
3. asks the asker to clarify the question, or
4. points to where the answer is (a doc, ticket, channel, or person).

Rules:
- Only pair IDs from "new_messages" with IDs from "open_questions". Never cite IDs from "earlier_messages" or invent IDs.
- Never pair a message written by the same author as the question's asker.
- Ignore unrelated chatter, greetings, status updates, and discussions about other topics.
- Set "confident": true ONLY when the message clearly and unambiguously responds to that specific question. Set "confident": false when the link is plausible or indirect.
- Put matches in draft.matches (empty array if none). Put brief remarks in notes if helpful.
