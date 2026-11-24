# Arcanum-RS — Codex Instructions

## Development workflow

- Focus on implementation, debugging, testing and benchmarks.
- Do not inspect Git state at the beginning of a task.
- Do not run Git commands unless explicitly requested or strictly necessary for a specific development issue.
- Never commit, push, merge, rebase, reset, create branches or open pull requests unless explicitly instructed.
- Assume the current working branch is already prepared.
- Leave implementation changes uncommitted for review.
- Minimize unnecessary repository administration, planning and status narration.
- Avoid expanding the requested milestone beyond its defined scope.
- Stop investigating once sufficient evidence exists to implement a verified correction.
- Prioritize functional improvements over lengthy reports and repetitive demonstrations.
- Avoid unnecessary token consumption.

## Project philosophy

- Arcanum-RS is an independent native Rust replacement engine, not a recreation of individual Arcanum locations.
- The engine must interpret original game data through generic, reusable systems.
- Arcanum-CE and TIG are read-only implementation references.
- Original game resources and verified original engine behavior are the primary sources of truth.
- Preserve compatibility with original game data and legitimate content overrides.
- Never commit original copyrighted game assets.
- Prefer functional gameplay over isolated technical demonstrations.
- Treat incomplete functionality as an explicit limitation rather than concealing it through workarounds.
- Preserve original gameplay semantics while allowing optional modern enhancements.
- Moddability, configurability, performance and maintainability are fundamental architectural requirements.
- Do not reproduce limitations of the original executable unless required for genuine content or gameplay compatibility.

## Architectural completeness before exhaustive fidelity

During foundational engine development, prioritize implementing missing mechanics and establishing coherent interactions between gameplay subsystems.

- Establish functional, extensible implementations grounded in original-data semantics.
- Resolve foundational decoding errors, crashes, data loss and architectural blockers immediately. Avoid arbitrary resource limits that discard legitimate original content; use justified, bounded preparation and explicit diagnostics.
- Do not pursue exhaustive behavioral or cosmetic fidelity while relevant dependent systems remain unimplemented.
- Record unresolved compatibility discrepancies and their dependencies through the existing diagnostic infrastructure.
- Extend machine-readable observability alongside each new subsystem, without allowing diagnostic refinement to dominate implementation.
- Avoid temporary assumptions and workarounds that duplicate functionality intended for future subsystems.
- Prefer completing interconnected gameplay foundations over repeatedly refining isolated mechanics. Prioritize missing dependencies over exhaustive fixes to their downstream symptoms.
- Maintain focused regression coverage and basic integration checks as systems are added; defer exhaustive cross-system fidelity audits until the relevant mechanics are connected.

Once the principal gameplay systems are implemented and interconnected, undertake dedicated comprehensive compatibility sweeps using the expanded machine-readable diagnostic infrastructure.

The objective is a coherent, extensible reconstruction of Arcanum, followed by systematic convergence toward original behavior.

## Generic world reconstruction

- The engine must reconstruct arbitrary original locations from installed game data without prior knowledge of their individual architecture.
- Treat location-specific defects as potential symptoms of missing or incorrect generic engine behavior.
- Trace defects to the responsible subsystem before implementing corrections.
- Derive terrain, sectors, walls, roofs, portals, scenery, mobiles, lighting, collision and transitions from original resources and verified interpretation rules.
- Reproduce original default-sector, wilderness and terrain-generation behavior wherever applicable.
- Determine original world-loading behavior from reference source and resources rather than assuming how it works.
- Do not invent terrain or geometry where original data or verified engine rules do not support it.
- Never manually assemble original buildings or environments.
- Never introduce location-specific rendering paths, coordinate overrides, object-placement corrections or artificial traversal exceptions to compensate for incomplete engine functionality.
- Map names, sector IDs, coordinates and object identifiers may be used in regression fixtures and diagnostics, but must not become special cases in general engine logic.
- Preserve genuine location-specific behavior encoded in original scripts, object properties and game resources.
- Use Arcanum-CE source to investigate missing format interpretation and runtime behavior rather than guessing from screenshots.
- Do not silently discard entire sectors or object collections because an individual record is unsupported.
- Preserve strict binary-format validation. Never skip unknown variable-length records using guessed offsets or lengths.
- When safe parsing cannot continue, report the failure explicitly rather than presenting an incomplete sector as correctly reconstructed.
- Report unsupported records, missing resources, incomplete collision data and unresolved rendering behavior.
- Maintain traceability from rendered and interactive objects back to their original records, prototypes, ART resources and applicable rules.
- Verify generic corrections against multiple original locations whenever practical, including previously untested content.
- Do not treat successful reconstruction of a small set of previously audited locations as proof of complete world-format support.

## Engineering principles

- Prefer reusable engine systems over milestone-specific demonstrations.
- Avoid speculative fixes, arbitrary offsets and hardcoded object positions.
- Prefer targeted investigation and minimal verified changes.
- Fix root causes rather than individual visual symptoms.
- Reuse established systems instead of introducing parallel implementations.
- Keep gameplay, world loading, rendering, input and presentation responsibilities appropriately separated.
- Gameplay and diagnostic viewers should share the same underlying world reconstruction and rendering systems.
- Preserve meaningful regression tests and performance baselines.
- Remove obsolete implementations once their replacements are verified.
- Avoid maintaining redundant systems solely for historical compatibility with earlier development milestones.
- Do not undertake broad architectural rewrites without a demonstrated technical requirement.
- Prefer explicit, maintainable abstractions over speculative generalization.
- Keep original compatibility behavior separate from optional engine enhancements.
- Avoid unnecessary dependencies, duplicated code and redundant compatibility layers.
- Keep implementation complexity proportional to actual engine requirements.

## Rendering architecture

- Vulkan is the sole production rendering backend for Arcanum-RS.
- Use the existing wgpu infrastructure configured exclusively for Vulkan.
- Do not introduce alternative production rendering backends unless explicitly instructed.
- Do not restore the retired CPU software renderer, OpenGL/GLES backend, automatic backend selection or their obsolete options, dependencies and feature flags.
- Do not silently fall back to software rendering or another graphics backend.
- Report Vulkan initialization failures with clear, actionable diagnostics.
- Preserve CPU-side asset decoding, world reconstruction, collision, gameplay logic and other appropriate non-graphics workloads.
- Retain isolated software reference functions only when necessary for meaningful automated tests or format verification.
- Do not retain a complete software renderer as a testing dependency.
- Integrate world rendering, lighting, shadows, animation, roof fading, map rendering and UI composition into one shared Vulkan rendering architecture.
- Prefer GPU execution for compositing, scaling, blending and other suitable graphics workloads.
- Avoid unnecessary GPU readbacks, repeated texture uploads and redundant CPU-side rasterization.
- Preserve efficient texture and resource caching.
- Prefer asynchronous asset preparation and sector streaming where practical.
- Maintain event-driven presentation and avoid unnecessary idle rendering.
- Prioritize frame pacing, low memory consumption and resource reuse.
- Maintain compatibility with low-end Vulkan-capable integrated GPUs, particularly Intel HD 520.
- Avoid unnecessary Vulkan feature requirements and excessive hardware assumptions.
- Validate rendering correctness against original game data, reference behavior and screenshots.
- Do not require pixel parity with retired rendering backends.
- Preserve meaningful asset-free and GPU-independent regression coverage.
- Keep hardware-dependent rendering tests focused on Vulkan.

## Moddability and configuration

- Treat moddability as a core architectural requirement.
- Prefer data-driven definitions and documented configuration over hardcoded content behavior.
- Preserve predictable VFS resource and mod override precedence.
- Allow externally supplied assets, content and configuration wherever supported.
- Do not require engine recompilation for ordinary asset, layout, font or theme modifications.
- Keep original game archives read-only during normal engine operation.
- Support externally supplied replacement resources without requiring modification of original archives.
- Keep extension points reusable for future scripting, mod management and developer-console functionality.
- Avoid unnecessary coupling between individual content packages and core engine code.
- Do not introduce a new configuration or extension mechanism when an existing one can reasonably be extended.
- Distinguish original compatibility behavior from optional modifications.
- Keep user configuration independent of individual savegames wherever appropriate.
- Ensure future launchers, configuration tools and the engine can share the same documented configuration format.
- Keep the engine independently launchable without requiring a graphical launcher.
- Do not introduce heavyweight dependencies or premature scripting abstractions without a concrete implementation requirement.
- Document public extension points, configuration formats and supported resource overrides.

## User interface architecture

Arcanum-RS is an independent replacement engine with its own interface design philosophy. Reproducing the original Arcanum interface is not a project requirement.

- Prioritize usability, readability, efficiency and coherent interaction design.
- Preserve original gameplay semantics independently of interface presentation.
- Modernize interaction conventions where beneficial.
- Avoid unnecessary modal windows, redundant controls, excessive navigation and screen-consuming decorative elements.
- Prefer compact, functional interfaces that maximize visible gameplay space.
- Use original item artwork and other resources where useful, without requiring original panel geometry or layouts.
- Support modern graphical inventories, drag-and-drop, contextual actions, tooltips and efficient item transfers.
- Keep gameplay state and transactions independent of presentation.
- Use a shared, externally configurable UI component system.
- Support independently replaceable HUDs and interface screens.
- Preserve independent UI scaling and world-camera zoom.
- Support external fonts, artwork, colors and layouts.
- Keep diagnostic interfaces separate from ordinary gameplay.
- Do not instantiate inactive interfaces as hidden duplicate overlays.
- Use the shared Vulkan rendering infrastructure.
- Do not require engine recompilation for ordinary interface customization.
- Treat 1920×1080 as a primary testing resolution while supporting other resolutions and aspect ratios.
- Original-style interfaces may be implemented as optional community themes, but are not mandatory development targets.
- Do not spend development time reproducing obsolete interface conventions solely for historical fidelity.

## Gameplay-driven development

- Prioritize a functional, explorable game over exhaustive visual perfection.
- Use actual gameplay to discover defects and validate engine behavior.
- Treat original locations as regression environments, not individual implementation projects.
- Fix discovered defects through generic engine rules wherever possible.
- Preserve legitimate original gameplay restrictions, including locked doors, travel eligibility and scripted progression.
- Do not bypass missing functionality through undocumented teleports, artificial unlocks or location-specific exceptions.
- Keep developer commands and testing conveniences explicitly separate from normal gameplay.
- Preserve the original Crash Site as the default starting location unless explicitly instructed otherwise.
- Support alternative original starting locations for development through documented commands or configuration.
- Prioritize systems that unlock further manual playtesting and natural exploration.
- Do not create additional standalone demonstrations when the same functionality can be verified through the playable engine.
- Do not repeatedly implement isolated visual features without integrating them into actual gameplay.
- Record nonblocking defects for subsequent investigation.
- Do not claim complete original gameplay compatibility when only a limited functional subset is implemented.

## Developer tools

- Prefer reusable runtime inspection and diagnostic systems over one-off debugging utilities.
- Keep developer functionality explicitly distinguishable from normal gameplay.
- Preserve original object and resource provenance in diagnostic output.
- Make relevant engine state inspectable without requiring source modifications.
- Reuse existing map-loading, interaction and rendering systems for developer commands.
- Extend the existing shared command registry rather than introducing another command parser.
- Developer commands must not require permanent changes to original content or gameplay rules.
- Prefer diagnostics that help identify root causes during actual gameplay.
- Do not introduce developer overlays into the default Classic interface.
- Keep developer tools lightweight and accessible from the playable engine wherever practical.

## Testing and reference material

- Prefer reproducible automated tests and focused native gameplay verification.
- Use existing Arcanum-CE reference screenshots in `~/Pictures/Screenshots/`, particularly filenames containing `arcanum-ce`.
- Consult reference source code and existing screenshots before launching the reference executable.
- Avoid repeatedly launching and closing Arcanum-CE when sufficient evidence has already been collected.
- Do not spend excessive time pursuing pixel-perfect cosmetic comparisons.
- Do not repeatedly reuse the same demonstration scene as the sole evidence of generic engine correctness.
- Use varied original locations to verify shared systems.
- Treat location-specific tests as regression fixtures for generic engine functionality.
- Test actual documented launch commands and supported argument combinations.
- Verify user-facing features through the playable executable rather than relying exclusively on internal tests.
- Prefer targeted regression tests during implementation and appropriate broader validation before milestone completion.
- Use Vulkan exclusively for production rendering validation.
- Do not perform redundant CPU, OpenGL or GLES rendering comparisons.
- Do not require physical GPU hardware for tests unrelated to rendering.
- Preserve meaningful performance and resource-consumption measurements.
- Avoid repeatedly running expensive full validation suites during small intermediate corrections unless necessary.
- Clearly distinguish verified original behavior from incomplete or approximate implementations.
- Never claim functionality has been verified if the corresponding test was not performed.
- Do not treat successful compilation or passing automated tests as proof that a feature works correctly during gameplay.
- Record unresolved issues honestly instead of introducing speculative fixes to satisfy acceptance criteria.

## Machine-readable diagnostics and automation

- Read [docs/AGENT_WORKFLOW.md](docs/AGENT_WORKFLOW.md) before debugging runtime behavior through simulated keyboard or mouse interaction.
- Use the local `rpc` client as the first tool for runtime reproduction and state inspection. Use the headless `world-scan` command for original-data compatibility investigations.
- Share the developer command registry across the console and automation interfaces.
- Preserve actionable source provenance and group compatibility failures by their underlying engine or format cause.
- Extend the existing diagnostics alongside each new or substantially expanded gameplay subsystem. Treat relevant state inspection, reproducible developer control and focused behavioral verification as part of implementation, not a later retrofit.
- Expose meaningful state, transitions, rejected operations and unresolved dependencies through structured results. Use the same gameplay mechanisms as normal play, not parallel testing implementations.
- Support reproducible diagnostic contexts through versioned snapshots; document what is and is not restored.
- Extend `world-scan` coverage where meaningful static verification becomes possible. Keep structural decoding, semantic interpretation, preparation and actual runtime behavior distinct.
- Preserve compatibility history and comparable baselines. Distinguish recovered content, diagnostic reclassification, newly evaluated coverage and regressions; never suppress warnings merely to reduce counts.
- Keep command discovery, result schemas, focused regression tests and the agent workflow documentation synchronized with implemented capabilities.
- Prefer headless validation for nonvisual behavior and screenshots for actual visual verification.
- Keep automation local, explicitly developer-enabled, disabled in ordinary play and independent of external AI providers.
- Use bounded event histories and on-demand capture. Avoid redundant readers, continuous diagnostic logging and unnecessary GPU readbacks.

## Completion reports

- Summarize implemented changes and verified behavior.
- Include relevant test results, measurements and remaining limitations.
- Explain the root cause of corrected defects and identify the reusable engine mechanism involved.
- Include original source-data or reference-code evidence for significant compatibility corrections.
- Provide screenshots for relevant visual rendering changes.
- Provide exact launch commands and controls for manual gameplay testing.
- Verify that documented launch commands work before reporting completion.
- Explicitly identify unsupported behavior and unresolved defects.
- Distinguish implemented functionality from proposed future work.
- Keep completion reports concise and factual.
- Do not repeat information already established by previous milestone reports.
- Do not perform Git integration.
