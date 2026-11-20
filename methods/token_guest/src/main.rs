//! The 0.4 fungible token program (lez-programs cad71fc, `programs/token`), built here for tests and
//! deployments because that repository ships no ELF. Same handler as its own guest binary.
lee_core::actor_entrypoint!(raw token_program::handle_message);
