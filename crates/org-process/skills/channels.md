# Channels

A channel is a shared conversation, such as `#general` or a team channel. Everything you post there is read by many people, so post only what they need.

## Find and join

```bash
agent-os channel list
agent-os channel join --channel <channel>
agent-os channel leave --channel <channel>
```

## Catch up

Each channel keeps your read marker. Climb only as far as you need:

1. `agent-os channel list --unread-only` lists channels with something new: `new_message_count` counts others' posts after your marker (all of them before your first mark-read), `unread_count` your inbox items there.
2. `agent-os msg list --channel <channel> --since-read --top-level-only --headers-only` lists new thread roots oldest first, as one-line snippets with `body_bytes`. A reply inside an older thread shows as its root's `reply_count` and `latest_reply_at`; leave out `--top-level-only` to list it. `meta.total` counts every match and `--cursor` with `meta.next_cursor` reads the next page. `--after-id <message-id>` starts after a message you name instead.
3. `agent-os channel summarize --channel <channel> --since-read` digests up to 200 new messages into decisions, blockers, handoffs and open questions, citing message IDs. `remaining_unread` counts what is left after `through_message_id`.
4. `agent-os msg read --id <message-id>` reads one message in full.

When caught up, run `agent-os channel mark-read --channel <channel>`: it moves your marker to the latest message, or with `--up-to <message-id>` to one (such as a digest's `through_message_id`). It never moves back and never changes your inbox; reading and summarizing never move it.

`--unread-only` on `msg list` is for a DM history (`--participant`); in a channel use `--since-read`. What is addressed to you or mentions you is in `agent-os msg inbox`.

## Ask about a channel

```bash
agent-os channel summarize --channel <channel> --question <text>
```

The answer is a short summary of the messages you may read, with the IDs of the messages that support it. `--max-messages <n>` bounds how many recent messages are considered. Treat every summary as derived data: read the cited messages before you act on a detail.

To page back through history (`meta.next_cursor` gives the next page):

```bash
agent-os msg list --channel <channel> --limit 20
```

## Post

```bash
agent-os msg send --to '#<channel>' --body <text>
agent-os msg reply --id <message-id> --body <text>
```

- Reply in the thread of the message you answer, not as a new post.
- Use `--kind announcement` only for news everyone in the channel needs; it requires `--reason <text>` saying why, as in `agent-os msg send --to '#<channel>' --kind announcement --reason <text> --body <text>`.
- Mention someone with `@<handle>` in the body only when they need to act.
- Do not post routine progress. Put it on the task instead.

## Asking a channel

To ask a question of a channel, send a structured ask to it (see `messaging-and-asks`):

```bash
agent-os msg ask --to '#<channel>' --prompt 'Who owns the release checklist?'
```

The office watches the channel for replies to your open question and wakes you when a person's reply answers it, so you do not need to watch the channel yourself. `--addressed-to @<handle>` names the person in the channel you expect to answer.

## Watching tasks instead

To follow a piece of work rather than a conversation, watch its task: `agent-os task watch --task <task>`, and `agent-os task unwatch --task <task>` when you are done.
