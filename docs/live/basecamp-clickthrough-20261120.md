# Basecamp click-through, 20 November 2026

Basecamp 0.3.2 (a test install with nothing else at stake), packages built from `cleanup` at 2ff50d6:
`vesting-0.2.0.lgx` sha256 `482cc93ef98a3e4957c442496662e0ce22a55b62bb23af8adfcdd629d4005483`,
`vesting_ui-0.2.0.lgx` sha256 `c624a5ae8ddca15f6fcd7ea9520377181a581ef15d282d00fd01f20a83ab2785`.
Chain: the public LEZ rc3 demo chain (`https://lez.nameswallet.app/`, chain id `2da7f7df…5374`),
Vest program `2kgxMAueD56gK2tArRA4ziFFw1Hz4W6njVZWGNJXHeM2`. A fresh test wallet: creator A
`FLBC4VrQ12UDpbG2ArnRgcwrZTz5cabKbT9oY1M7BE2Z`, beneficiary B `76DoKtAcVM98avrHdPV7PiUgzCz5mAUJWJKdv5Tbj8ub`.

| Step | Result |
|---|---|
| Install both packages | installed; the module loads (after f1833e4: native libraries linked) |
| Settings: wallet folder, Save and connect | "Connected to Bridge test network (LEZ rc3 demo chain)" (after cd1ff52: a typed value is committed before the click) |
| Creator: accounts | the wallet's three public accounts with live balances |
| Create: linear, LEZ, 2026-11-19 → 2026-11-22, A → B, 1,000,000 | schedule 0, tx `08a7a7e970b5ec585fff05d1b32695af66388e6e7deee067736ffe95a7931a68`, block 39975 |
| Recipient: position | kind, account, total, locked, claimed, chain time, claimable now, next unlock |
| Claim (public): pre-claim summary | claimable 0.000652536 LEZ, estimated fee 0.00033712 LEZ, "Your public account can pay the estimated fee" (after 2ff50d6: ids sent as text) |
| Confirm claim | "Claimed 0.000652652 LEZ to 5a7c7f…9a56 (public)", tx `2a5605…b404`, block 40153 |
| Cancel (creator) | "Cancelled schedule 0. Refunded 0.000347117 LEZ to the creator." |

Bugs found and fixed on the way: the plugin did not load (PC/SC and other native libraries not
linked, f1833e4); a value typed just before a button click was lost (cd1ff52); Basecamp refused every
call with an integer argument ("expected integer at arg0, got number"), now sent as text (2ff50d6).

Not tried here: a private claim (a real proof of several minutes on this machine), milestone signal,
transfer, batch, LNS names in the form.

Noticed, not yet fixed: after a cancel, the schedule's Cancel and Make irrevocable buttons stay until
the next refresh; Cancel has no confirm step; after a package reinstall the app opens "not opened yet"
although the settings were kept.
