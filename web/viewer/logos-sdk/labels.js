// Known programs on the LEZ 0.4 bridge test network. Adapted from the site explorer's labels.js
// (checked 2026-11-19), which cites explorer-src/CLAIMS.md for each entry. A program registered
// in the LEZ Program Registry should be named from the chain instead; these are hints only.
// SPDX-License-Identifier: MIT OR Apache-2.0

export const REGISTRY_PROGRAM = '7sGWX1JTYoD2GFs69sWdX9PqJzhpcdk8ouZBu3BYeY82';
export const NATIVE_PROGRAM = '11111111111111111111111111111111';
export const LOADER_PROGRAM = 'JAQ5MVHbCkSYRzXunsrNuM2m1LS859PGveHfoYPAmcvZ';
export const FEE_PROGRAM = '2TsGdKDWTY92X4vSBZzjzemZpThhyqNEdZJj3xBrQ9AZ';
export const CLOCK_PROGRAM = '2hwaRW8sMnyKLLRbDRw8k68tiv9bnfq98oxcdtcp157Z';
export const LNS_PROGRAM = '6Yxj5k8AiuBSuMAYVQFGrYKjJGUSSaWWgoB9D9XXN3gA';

const BUILT_IN = 'Built into LEZ';
const OURS = 'Deployed by our team';

export const LABELS = {
  [NATIVE_PROGRAM]: { name: 'Native coin', by: BUILT_IN },
  [LOADER_PROGRAM]: { name: 'Program loader', by: BUILT_IN },
  [FEE_PROGRAM]: { name: 'Fees', by: BUILT_IN },
  [CLOCK_PROGRAM]: { name: 'Clock', by: BUILT_IN },
  '5NbpF2r22SE97w1PhFcy5JBwfRrRrUvJToLzUR94EtyW': { name: 'Sequencer stake', by: BUILT_IN },
  [REGISTRY_PROGRAM]: { name: 'LEZ Program Registry', by: OURS },
  [LNS_PROGRAM]: { name: 'LNS names registry', by: OURS },
};

export const labelOf = (program) => LABELS[program] || null;
