// DOM wiring for the static vesting viewer. All chain reads go through the vendored logos-sdk-js;
// all decoding and maths are in logic.mjs. No build step, no writes.
// SPDX-License-Identifier: MIT OR Apache-2.0

import { Client } from './logos-sdk/rpc.js';
import { clock, labels, lns } from './logos-sdk/index.js';
import {
  amountText, classifyInput, decodeScheduleState, decodeVestingEvent, describeSchedule,
  scheduleAddress,
} from './logic.mjs';

function actorState(value, program) {
  const states = value?.data?.actor_states ?? value?.data?.shards ?? {};
  const bytes = states[program];
  return bytes ? Uint8Array.from(bytes) : null;
}

async function readState(client, program, account) {
  const value = await client.call('getAccountView', [{ account_id: account, program_account_id: program }]);
  const bytes = actorState(value, program);
  return bytes ? decodeScheduleState(bytes) : null;
}

/** Event history is best effort: many rc3 RPCs do not serve getEvents at all. */
async function readEvents(client, program, schedule) {
  const shapes = [[program], [{ program_account_id: program }], []];
  for (const params of shapes) {
    try {
      const result = await client.call('getEvents', params);
      const groups = Array.isArray(result) ? result : [];
      const entries = groups.flatMap((group) => (Array.isArray(group?.events) ? group.events : [group]));
      const events = [];
      for (const entry of entries) {
        const eventProgram = entry?.account_id ?? entry?.program_account_id;
        if (eventProgram && eventProgram !== program) continue;
        if (!entry?.event?.data) continue;
        try {
          const decoded = decodeVestingEvent(Uint8Array.from(entry.event.data));
          if (decoded.schedule === schedule) events.push(decoded.event);
        } catch { /* not one of our events */ }
      }
      if (events.length) return { available: true, events };
    } catch { /* this RPC does not serve getEvents; try the next shape */ }
  }
  return { available: false, events: [] };
}

function table(rows) {
  const element = document.createElement('table');
  for (const [key, value] of rows) {
    const row = element.insertRow();
    const name = row.insertCell(); name.textContent = key;
    const cell = row.insertCell(); cell.textContent = String(value);
  }
  return element;
}

function showSchedule(schedule, details, history, container) {
  const card = document.createElement('section');
  card.className = 'card';
  card.append(table([
    ['Kind', details.kind], ['Total', details.total], ['Unlocked now', details.vested],
    ['Claimed', details.claimed], ['Claimable', details.claimable], ['Locked', details.locked],
    ['Next unlock', details.nextUnlock], ['Cancelled', details.cancelled],
    ['Transferable', details.transferable], ['Creator', details.creator],
    ['Receives', details.beneficiary], ['Network time', details.lastTime],
  ]));
  const heading = document.createElement('h2');
  heading.textContent = 'Event history';
  card.append(heading);
  if (!history.available) {
    const note = document.createElement('p');
    note.className = 'note';
    note.textContent = 'This node cannot show past events; the schedule above still comes from the chain.';
    card.append(note);
  } else if (history.events.length === 0) {
    card.append(Object.assign(document.createElement('p'), { textContent: 'No events for this schedule.' }));
  } else {
    const list = document.createElement('ul');
    for (const event of history.events) {
      const item = document.createElement('li');
      item.textContent = `${event.name} at ${new Date(Number(event.at)).toISOString()}${event.amount !== undefined ? ` · ${amountText(event.amount)}` : ''}${event.toPrivate !== undefined ? ` · ${event.toPrivate ? 'private' : 'public'}` : ''}`;
      list.append(item);
    }
    card.append(list);
  }
  container.append(card);
}

export async function run({ rpcUrl, program, input, nonce, status, result }) {
  const client = new Client({ url: rpcUrl, ratePerSec: 4, burst: 1 });
  const kind = classifyInput(input);
  let account = kind.value;

  if (kind.type === 'name') {
    status.textContent = `Resolving ${input}…`;
    const record = await lns.resolveName(client, labels.LNS_PROGRAM, input);
    account = lns.publicView(record)?.publicAccount;
    if (!account) throw new Error(`No public account is published on that LNS name. Enter the address instead.`);
  }

  status.textContent = 'Reading chain state…';
  let state = await readState(client, program, account);
  if (!state && nonce) {
    status.textContent = 'Finding the schedule from its creator…';
    account = await scheduleAddress(program, account, nonce);
    state = await readState(client, program, account);
  }
  if (!state) {
    throw new Error('No schedule found at that address. Enter its address, or the creator and label.');
  }
  if (!state.schedule) throw new Error('The schedule is still being created. Try again in a moment.');

  status.textContent = 'Reading clock and events…';
  let now = Date.now();
  try { now = (await clock.read(client)).timestamp; } catch { /* fall back to local time */ }
  const history = await readEvents(client, program, account);
  result.replaceChildren();
  showSchedule(state.schedule, describeSchedule(state.schedule, now), history, result);
  status.textContent = `Account ${account} · program ${program}`;
}
