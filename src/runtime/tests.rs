use super::*;
use crate::content::effects::EffectId;
use crate::content::items::ResistanceKind;

fn item() -> GenericDefinition {
    GenericDefinition {
        id: ItemId("smithing:item/charm".into()),
        display_name: "Charm".into(),
        base_weight: 1,
        base_worth: 3,
        magic_tech_complexity: 0,
    }
}

fn additive(id: &str, points: i32) -> EffectDefinition {
    EffectDefinition {
        id: EffectId(id.into()),
        operations: vec![EffectOperation::AddArmourClass { points }],
    }
}

fn fire_floor(id: &str, minimum_percent: i32) -> EffectDefinition {
    EffectDefinition {
        id: EffectId(id.into()),
        operations: vec![EffectOperation::ResistanceFloor {
            channel: ResistanceKind::Fire,
            minimum_percent,
        }],
    }
}

fn electrical(id: &str, percentage_points: i32) -> EffectDefinition {
    EffectDefinition {
        id: EffectId(id.into()),
        operations: vec![EffectOperation::AddResistance {
            channel: ResistanceKind::Electrical,
            percentage_points,
        }],
    }
}

fn equip<'a>(
    runtime: &mut Runtime<'a>,
    actor: ActorId,
    item: &'a GenericDefinition,
) -> ItemInstanceId {
    let instance = runtime
        .instantiate_item(ItemDefinitionRef::Generic(item))
        .unwrap();
    runtime.equip(instance, actor).unwrap();
    instance
}

#[test]
fn native_item_and_effect_work_without_legacy_content() {
    let item = item();
    let effect = additive("smithing:effect/ward", 1);
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    assert_eq!(runtime.base_armour_class(actor), Ok(10));
    assert_eq!(runtime.armour_class(actor), Ok(10));
    let source = equip(&mut runtime, actor, &item);
    assert_eq!(runtime.item(source).unwrap().definition.id(), &item.id);
    assert_eq!(runtime.armour_class(actor), Ok(10));

    let id = runtime.activate_effect(source, actor, &effect).unwrap();
    let grant = runtime.activation(id).unwrap();
    assert_eq!((grant.id, grant.source, grant.target), (id, source, actor));
    assert_eq!(grant.definition.id, effect.id);
    assert_eq!(grant.armour_class_points, 1);
    assert_eq!(runtime.armour_class(actor), Ok(11));

    let other = runtime.activate_effect(source, actor, &effect).unwrap();
    assert_ne!(id, other);
    assert_eq!(runtime.armour_class(actor), Ok(12));
    runtime.remove_activation(other).unwrap();
    assert_eq!(runtime.armour_class(actor), Ok(11));
    assert_eq!(runtime.activation(id).unwrap().source, source);
    assert_eq!(
        runtime.remove_activation(other),
        Err(Error::UnknownActivation(other))
    );
    assert_eq!(runtime.item(source).unwrap().equipped_by, Some(actor));
    runtime.remove_activation(id).unwrap();
    assert_eq!(runtime.armour_class(actor), Ok(10));
}

#[test]
fn unequip_uses_recorded_grants_after_character_state_changes() {
    let item = item();
    let effect = additive("arcanum:effect/128", 1);
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    let first = equip(&mut runtime, actor, &item);
    let second = equip(&mut runtime, actor, &item);
    assert_ne!(first, second);
    // Supplied counts from the Sword of Defense trace. The runtime receives
    // only the grants, independent of the caller's later aptitude context.
    let remaining = runtime.activate_effect(first, actor, &effect).unwrap();
    let grants: Vec<_> = (0..15)
        .map(|_| runtime.activate_effect(second, actor, &effect).unwrap())
        .collect();
    assert_eq!(runtime.armour_class(actor), Ok(26));
    runtime.set_base_armour_class(actor, 20).unwrap();
    assert_eq!(runtime.armour_class(actor), Ok(36));

    runtime.unequip(second, actor).unwrap();
    assert_eq!(runtime.armour_class(actor), Ok(21));
    assert!(grants.iter().all(|id| runtime.activation(*id).is_err()));
    assert_eq!(
        runtime.activation(remaining).unwrap().armour_class_points,
        1
    );
    assert_eq!(runtime.item(second).unwrap().equipped_by, None);
    runtime.unequip(first, actor).unwrap();
    assert_eq!(runtime.armour_class(actor), Ok(20));
    assert!(runtime.activation(remaining).is_err());
}

#[test]
fn equipment_is_exclusive_and_invalid_identities_fail() {
    let item = item();
    let effect = additive("arcanum:effect/128", 1);
    let mut runtime = Runtime::default();
    let first = runtime.create_actor(10).unwrap();
    let second = runtime.create_actor(40).unwrap();
    let source = equip(&mut runtime, first, &item);
    let unknown_actor = ActorId(u64::MAX);
    let unknown_item = ItemInstanceId(u64::MAX);
    assert_eq!(
        runtime.activate_effect(source, unknown_actor, &effect),
        Err(Error::UnknownActor(unknown_actor))
    );
    assert_eq!(
        runtime.activate_effect(unknown_item, first, &effect),
        Err(Error::UnknownItem(unknown_item))
    );
    for actor in [first, second] {
        assert_eq!(
            runtime.equip(source, actor),
            Err(Error::AlreadyEquipped {
                item: source,
                wearer: first
            })
        );
    }
    assert_eq!(
        runtime.activate_effect(source, second, &effect),
        Err(Error::NotEquippedBy {
            item: source,
            actor: second
        })
    );
    assert_eq!(
        runtime.unequip(source, second),
        Err(Error::NotEquippedBy {
            item: source,
            actor: second
        })
    );

    let id = runtime.activate_effect(source, first, &effect).unwrap();
    assert_eq!(runtime.armour_class(first), Ok(11));
    assert_eq!(runtime.armour_class(second), Ok(40));
    runtime.unequip(source, first).unwrap();
    assert_eq!(
        runtime.unequip(source, first),
        Err(Error::NotEquippedBy {
            item: source,
            actor: first
        })
    );
    assert_eq!(
        runtime.activate_effect(source, first, &effect),
        Err(Error::NotEquippedBy {
            item: source,
            actor: first
        })
    );
    runtime.equip(source, second).unwrap();
    let replacement = runtime.activate_effect(source, second, &effect).unwrap();
    assert_ne!(id, replacement);
    assert_eq!(runtime.armour_class(first), Ok(10));
    assert_eq!(runtime.armour_class(second), Ok(41));
}

#[test]
fn unsupported_composite_reports_error_without_partial_application() {
    let item = item();
    let mut effect = additive("smithing:effect/ward", 1);
    let unsupported = EffectOperation::AddResistance {
        channel: ResistanceKind::Fire,
        percentage_points: 8,
    };
    effect
        .operations
        .extend(fire_floor("arcanum:effect/53", 50).operations);
    effect.operations.push(unsupported);
    let existing = fire_floor("smithing:effect/existing", 30);
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    let source = equip(&mut runtime, actor, &item);
    runtime.set_underlying_fire_resistance(actor, 20).unwrap();
    let id = runtime.activate_effect(source, actor, &existing).unwrap();
    assert_eq!(
        runtime.activate_effect(source, actor, &effect),
        Err(Error::UnsupportedOperation(unsupported))
    );
    assert_eq!(runtime.armour_class(actor), Ok(10));
    assert_eq!(runtime.fire_resistance(actor), Ok(30));
    assert_eq!(runtime.activation(id).unwrap().source, source);
    assert_eq!(runtime.activations.len(), 1);
    assert_eq!(runtime.item(source).unwrap().equipped_by, Some(actor));
}

#[test]
fn electrical_sources_can_be_removed_independently_without_cross_actor_leakage() {
    let item = item();
    let effect = electrical("smithing:effect/conductor", 8);
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    let other = runtime.create_actor(20).unwrap();
    runtime
        .set_underlying_electrical_resistance(actor, 30)
        .unwrap();
    runtime
        .set_underlying_electrical_resistance(other, 12)
        .unwrap();
    let first = equip(&mut runtime, actor, &item);
    let second = equip(&mut runtime, actor, &item);
    let first_grant = runtime.activate_effect(first, actor, &effect).unwrap();
    assert_eq!(runtime.electrical_resistance_after_effects(actor), Ok(38));
    assert_eq!(
        runtime.activation(first_grant).unwrap().definition.id,
        effect.id
    );
    let second_grant = runtime.activate_effect(second, actor, &effect).unwrap();
    assert_ne!(first_grant, second_grant);
    assert_eq!(runtime.electrical_resistance_after_effects(actor), Ok(46));
    assert_eq!(runtime.electrical_resistance(other), Ok(12));
    assert_eq!(
        runtime.activate_effect(first, other, &effect),
        Err(Error::NotEquippedBy {
            item: first,
            actor: other
        })
    );

    runtime.remove_activation(second_grant).unwrap();
    assert_eq!(
        runtime.remove_activation(second_grant),
        Err(Error::UnknownActivation(second_grant))
    );
    assert_eq!(runtime.item(second).unwrap().equipped_by, Some(actor));
    let surviving = runtime.activation(first_grant).unwrap();
    assert_eq!(
        (
            surviving.source,
            surviving.target,
            surviving.electrical_resistance_points
        ),
        (first, actor, 8)
    );
    assert_eq!(runtime.electrical_resistance_after_effects(actor), Ok(38));
    runtime.unequip(first, actor).unwrap();
    assert_eq!(runtime.electrical_resistance_after_effects(actor), Ok(30));
    assert_eq!(runtime.electrical_resistance(other), Ok(12));
}

#[test]
fn electrical_adjustments_follow_underlying_values_with_separate_query_clamping() {
    let item = item();
    let plus = electrical("smithing:effect/conductor", 8);
    let minus = electrical("smithing:effect/drain", -10);
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    let first = equip(&mut runtime, actor, &item);
    let second = equip(&mut runtime, actor, &item);
    runtime
        .set_underlying_electrical_resistance(actor, 30)
        .unwrap();
    runtime.activate_effect(first, actor, &plus).unwrap();
    runtime.activate_effect(second, actor, &plus).unwrap();
    runtime.unequip(first, actor).unwrap();
    assert_eq!(runtime.electrical_resistance_after_effects(actor), Ok(38));
    runtime
        .set_underlying_electrical_resistance(actor, 92)
        .unwrap();
    assert_eq!(runtime.underlying_electrical_resistance(actor), Ok(92));
    assert_eq!(runtime.electrical_resistance_after_effects(actor), Ok(100));
    assert_eq!(runtime.electrical_resistance(actor), Ok(95));
    runtime.unequip(second, actor).unwrap();
    assert_eq!(runtime.electrical_resistance(actor), Ok(92));

    runtime.equip(second, actor).unwrap();
    runtime
        .set_underlying_electrical_resistance(actor, 3)
        .unwrap();
    runtime.activate_effect(second, actor, &minus).unwrap();
    assert_eq!(runtime.electrical_resistance_after_effects(actor), Ok(-7));
    assert_eq!(runtime.electrical_resistance(actor), Ok(0));
    runtime.unequip(second, actor).unwrap();
    assert_eq!(runtime.electrical_resistance(actor), Ok(3));
}

fn body_of_fire() -> EffectDefinition {
    EffectDefinition {
        id: EffectId("arcanum:effect/54".into()),
        operations: crate::content::effects::decode_effect(
            "resistfire min 95, resistelectrical +8, resistpoison min 97",
        )
        .unwrap(),
    }
}

fn intermediate(runtime: &Runtime<'_>, actor: ActorId) -> [i32; 3] {
    [
        runtime.fire_resistance_after_effects(actor).unwrap(),
        runtime.electrical_resistance_after_effects(actor).unwrap(),
        runtime.poison_resistance_after_effects(actor).unwrap(),
    ]
}

#[test]
fn unsupported_operation_rejects_all_clauses_of_a_composite() {
    let item = item();
    let mut composite = body_of_fire();
    let unsupported = EffectOperation::ResistanceFloor {
        channel: ResistanceKind::Electrical,
        minimum_percent: 60,
    };
    composite.operations.push(unsupported);
    let existing = fire_floor("arcanum:effect/53", 50);
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    runtime
        .set_underlying_electrical_resistance(actor, 30)
        .unwrap();
    let first = equip(&mut runtime, actor, &item);
    let second = equip(&mut runtime, actor, &item);
    let floor = runtime.activate_effect(first, actor, &existing).unwrap();
    assert_eq!(
        runtime.activate_effect(second, actor, &composite),
        Err(Error::UnsupportedOperation(unsupported))
    );
    assert_eq!(intermediate(&runtime, actor), [50, 30, 0]);
    assert_eq!(runtime.armour_class(actor), Ok(10));
    assert_eq!(runtime.activation(floor).unwrap().source, first);
    assert_eq!(runtime.activations.len(), 1);
    assert_eq!(runtime.item(second).unwrap().equipped_by, Some(actor));
    let unknown_actor = ActorId(u64::MAX);
    let unknown_item = ItemInstanceId(u64::MAX);
    assert_eq!(
        runtime.activate_effect(second, unknown_actor, &composite),
        Err(Error::UnknownActor(unknown_actor))
    );
    assert_eq!(
        runtime.activate_effect(unknown_item, actor, &composite),
        Err(Error::UnknownItem(unknown_item))
    );
}

#[test]
fn poison_floor_follows_the_underlying_value_and_preserves_the_intermediate() {
    let item = item();
    let effect = EffectDefinition {
        id: EffectId("smithing:effect/poison-ward".into()),
        operations: vec![EffectOperation::ResistanceFloor {
            channel: ResistanceKind::Poison,
            minimum_percent: 97,
        }],
    };
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    let source = equip(&mut runtime, actor, &item);
    let id = runtime.activate_effect(source, actor, &effect).unwrap();
    for (underlying, expected) in [(20, 97), (90, 97), (97, 97), (99, 99)] {
        runtime
            .set_underlying_poison_resistance(actor, underlying)
            .unwrap();
        assert_eq!(runtime.underlying_poison_resistance(actor), Ok(underlying));
        assert_eq!(runtime.poison_resistance_after_effects(actor), Ok(expected));
        assert_eq!(runtime.poison_resistance(actor), Ok(95));
        assert_eq!(
            runtime.activation(id).unwrap().poison_resistance_floor,
            Some(97)
        );
    }
    runtime.remove_activation(id).unwrap();
    assert_eq!(runtime.poison_resistance_after_effects(actor), Ok(99));
    assert_eq!(runtime.poison_resistance(actor), Ok(95));
}

#[test]
fn independent_composite_sources_repeat_without_residual_operations() {
    let item = item();
    let effect = body_of_fire();
    let mut reordered = effect.clone();
    reordered.operations.reverse();
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    let other = runtime.create_actor(10).unwrap();
    runtime.set_underlying_fire_resistance(actor, 20).unwrap();
    runtime
        .set_underlying_electrical_resistance(actor, 30)
        .unwrap();
    runtime.set_underlying_poison_resistance(actor, 10).unwrap();
    let sources = [
        runtime
            .instantiate_item(ItemDefinitionRef::Generic(&item))
            .unwrap(),
        runtime
            .instantiate_item(ItemDefinitionRef::Generic(&item))
            .unwrap(),
    ];
    for (removed, kept) in [(0, 1), (1, 0)] {
        let mut grants = Vec::new();
        for (source, definition) in sources.into_iter().zip([&effect, &reordered]) {
            runtime.equip(source, actor).unwrap();
            grants.push(runtime.activate_effect(source, actor, definition).unwrap());
            let expected = if grants.len() == 1 { 38 } else { 46 };
            assert_eq!(intermediate(&runtime, actor), [95, expected, 97]);
        }
        assert_ne!(grants[0], grants[1]);
        let grant = runtime.activation(grants[0]).unwrap();
        assert_eq!(
            (grant.id, grant.target, grant.source),
            (grants[0], actor, sources[0])
        );
        assert_eq!(grant.definition, &effect);
        assert_eq!(
            (
                grant.fire_resistance_floor,
                grant.electrical_resistance_points,
                grant.poison_resistance_floor
            ),
            (Some(95), 8, Some(97))
        );
        assert_eq!(runtime.poison_resistance(actor), Ok(95));
        runtime.unequip(sources[removed], actor).unwrap();
        assert!(matches!(
            runtime.activation(grants[removed]),
            Err(Error::UnknownActivation(_))
        ));
        assert_eq!(
            runtime.activation(grants[kept]).unwrap().source,
            sources[kept]
        );
        assert_eq!(intermediate(&runtime, actor), [95, 38, 97]);
        assert_eq!(intermediate(&runtime, other), [0, 0, 0]);

        runtime.set_underlying_fire_resistance(actor, 98).unwrap();
        runtime
            .set_underlying_electrical_resistance(actor, 92)
            .unwrap();
        runtime.set_underlying_poison_resistance(actor, 90).unwrap();
        assert_eq!(intermediate(&runtime, actor), [98, 100, 97]);
        assert_eq!(
            [
                runtime.fire_resistance(actor),
                runtime.electrical_resistance(actor),
                runtime.poison_resistance(actor)
            ],
            [Ok(95), Ok(95), Ok(95)]
        );
        runtime.remove_activation(grants[kept]).unwrap();
        assert_eq!(
            runtime.remove_activation(grants[kept]),
            Err(Error::UnknownActivation(grants[kept]))
        );
        assert_eq!(intermediate(&runtime, actor), [98, 92, 90]);
        assert_eq!(runtime.armour_class(actor), Ok(10));
        runtime.unequip(sources[kept], actor).unwrap();
        assert!(runtime.activations.is_empty());
        runtime.set_underlying_fire_resistance(actor, 20).unwrap();
        runtime
            .set_underlying_electrical_resistance(actor, 30)
            .unwrap();
        runtime.set_underlying_poison_resistance(actor, 10).unwrap();
    }
}

#[test]
fn electrical_overflow_blocks_composite_activation_and_removal_atomically() {
    let item = item();
    let effect = body_of_fire();
    let cancelling = electrical("smithing:effect/drain", -8);
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    let source = equip(&mut runtime, actor, &item);
    let other = equip(&mut runtime, actor, &item);
    runtime
        .set_underlying_electrical_resistance(actor, i32::MAX)
        .unwrap();
    assert_eq!(
        runtime.activate_effect(source, actor, &effect),
        Err(Error::ElectricalResistanceOverflow)
    );
    assert_eq!(intermediate(&runtime, actor), [0, i32::MAX, 0]);
    assert!(runtime.activations.is_empty());
    assert_eq!(runtime.item(source).unwrap().equipped_by, Some(actor));

    runtime
        .set_underlying_electrical_resistance(actor, i32::MIN)
        .unwrap();
    let id = runtime.activate_effect(source, actor, &effect).unwrap();
    let drain = runtime.activate_effect(other, actor, &cancelling).unwrap();
    assert_eq!(
        runtime.remove_activation(id),
        Err(Error::ElectricalResistanceOverflow)
    );
    assert_eq!(
        runtime.unequip(source, actor),
        Err(Error::ElectricalResistanceOverflow)
    );
    assert_eq!(intermediate(&runtime, actor), [95, i32::MIN, 97]);
    assert_eq!(runtime.activation(id).unwrap().source, source);
    assert_eq!(runtime.item(source).unwrap().equipped_by, Some(actor));
    runtime.remove_activation(drain).unwrap();
    runtime.unequip(source, actor).unwrap();
    assert_eq!(intermediate(&runtime, actor), [0, i32::MIN, 0]);
}

#[test]
fn electrical_overflow_preserves_actively_cancelled_grants_and_actor_state() {
    let item = item();
    let plus = electrical("smithing:effect/conductor", 1);
    let minus = electrical("smithing:effect/drain", -1);
    let mut mixed = electrical("smithing:effect/mixed", 1);
    mixed
        .operations
        .push(EffectOperation::AddArmourClass { points: 3 });
    let mut oversized = electrical("smithing:effect/oversized", i32::MAX);
    oversized.operations.extend(plus.operations.iter().copied());
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    let first = equip(&mut runtime, actor, &item);
    let second = equip(&mut runtime, actor, &item);
    runtime
        .set_underlying_electrical_resistance(actor, i32::MAX)
        .unwrap();
    assert_eq!(
        runtime.activate_effect(first, actor, &mixed),
        Err(Error::ElectricalResistanceOverflow)
    );
    assert_eq!(runtime.armour_class(actor), Ok(10));
    assert!(runtime.activations.is_empty());
    assert_eq!(runtime.item(first).unwrap().equipped_by, Some(actor));
    let negative = runtime.activate_effect(first, actor, &minus).unwrap();
    assert_eq!(
        runtime.activate_effect(second, actor, &oversized),
        Err(Error::ElectricalResistanceOverflow)
    );
    let positive = runtime.activate_effect(second, actor, &plus).unwrap();
    assert_eq!(
        runtime.electrical_resistance_after_effects(actor),
        Ok(i32::MAX)
    );
    assert_eq!(
        runtime.remove_activation(negative),
        Err(Error::ElectricalResistanceOverflow)
    );
    assert_eq!(
        runtime.unequip(first, actor),
        Err(Error::ElectricalResistanceOverflow)
    );
    assert_eq!(runtime.activation(negative).unwrap().source, first);
    assert_eq!(runtime.item(first).unwrap().equipped_by, Some(actor));
    runtime.remove_activation(positive).unwrap();
    assert_eq!(
        runtime.set_underlying_electrical_resistance(actor, i32::MIN),
        Err(Error::ElectricalResistanceOverflow)
    );
    assert_eq!(
        runtime.underlying_electrical_resistance(actor),
        Ok(i32::MAX)
    );
    runtime.unequip(first, actor).unwrap();
    assert_eq!(
        runtime.electrical_resistance_after_effects(actor),
        Ok(i32::MAX)
    );
}

#[test]
fn fire_floor_uses_current_underlying_resistance() {
    let item = item();
    let effect = fire_floor("arcanum:effect/53", 50);
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    let source = equip(&mut runtime, actor, &item);
    runtime.set_underlying_fire_resistance(actor, 20).unwrap();
    assert_eq!(runtime.fire_resistance(actor), Ok(20));
    let id = runtime.activate_effect(source, actor, &effect).unwrap();
    for (underlying, expected) in [(20, 50), (40, 50), (50, 50), (70, 70)] {
        runtime
            .set_underlying_fire_resistance(actor, underlying)
            .unwrap();
        assert_eq!(runtime.underlying_fire_resistance(actor), Ok(underlying));
        assert_eq!(runtime.fire_resistance_after_effects(actor), Ok(expected));
        assert_eq!(runtime.fire_resistance(actor), Ok(expected));
        assert_eq!(
            runtime.activation(id).unwrap().fire_resistance_floor,
            Some(50)
        );
    }
    runtime.unequip(source, actor).unwrap();
    assert_eq!(runtime.fire_resistance(actor), Ok(70));
}

#[test]
fn independent_fire_floors_survive_removal_in_either_order() {
    let item = item();
    let effect = fire_floor("arcanum:effect/53", 50);
    for (removed, kept) in [(0, 1), (1, 0)] {
        let mut runtime = Runtime::default();
        let actor = runtime.create_actor(10).unwrap();
        let other = runtime.create_actor(10).unwrap();
        runtime.set_underlying_fire_resistance(actor, 20).unwrap();
        runtime.set_underlying_fire_resistance(other, 15).unwrap();
        let sources = [
            equip(&mut runtime, actor, &item),
            equip(&mut runtime, actor, &item),
        ];
        let grants = sources.map(|source| runtime.activate_effect(source, actor, &effect).unwrap());
        assert_ne!(grants[0], grants[1]);
        assert_eq!(runtime.fire_resistance(actor), Ok(50));
        runtime.unequip(sources[removed], actor).unwrap();
        assert!(matches!(
            runtime.activation(grants[removed]),
            Err(Error::UnknownActivation(_))
        ));
        let remaining = runtime.activation(grants[kept]).unwrap();
        assert_eq!(remaining.source, sources[kept]);
        assert_eq!(remaining.target, actor);
        assert_eq!(runtime.fire_resistance(actor), Ok(50));
        assert_eq!(runtime.fire_resistance(other), Ok(15));
        runtime.unequip(sources[kept], actor).unwrap();
        assert_eq!(runtime.fire_resistance(actor), Ok(20));
    }
}

#[test]
fn signed_underlying_resistance_and_native_floors_stay_unclamped_until_query() {
    let item = item();
    let zero = fire_floor("smithing:effect/zero", 0);
    let high = fire_floor("smithing:effect/fire-ward", 120);
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    runtime.set_underlying_fire_resistance(actor, -20).unwrap();
    let source = equip(&mut runtime, actor, &item);
    assert_eq!(runtime.fire_resistance_after_effects(actor), Ok(-20));
    assert_eq!(runtime.fire_resistance(actor), Ok(0));
    let zero_id = runtime.activate_effect(source, actor, &zero).unwrap();
    assert_eq!(runtime.fire_resistance_after_effects(actor), Ok(0));
    let high_id = runtime.activate_effect(source, actor, &high).unwrap();
    assert_eq!(runtime.activation(high_id).unwrap().definition.id, high.id);
    assert_eq!(runtime.underlying_fire_resistance(actor), Ok(-20));
    assert_eq!(runtime.fire_resistance_after_effects(actor), Ok(120));
    assert_eq!(runtime.fire_resistance(actor), Ok(95));
    runtime.remove_activation(high_id).unwrap();
    assert_eq!(runtime.fire_resistance_after_effects(actor), Ok(0));
    runtime.remove_activation(zero_id).unwrap();
    assert_eq!(runtime.fire_resistance_after_effects(actor), Ok(-20));
    runtime.set_underlying_fire_resistance(actor, 100).unwrap();
    assert_eq!(runtime.fire_resistance_after_effects(actor), Ok(100));
    assert_eq!(runtime.fire_resistance(actor), Ok(95));
}

#[test]
fn fire_floor_rejects_invalid_handles_and_other_channels() {
    let item = item();
    let effect = fire_floor("arcanum:effect/53", 50);
    let unsupported = EffectOperation::ResistanceFloor {
        channel: ResistanceKind::Electrical,
        minimum_percent: 50,
    };
    let mut composite = fire_floor("smithing:effect/unsupported", 95);
    composite.operations.push(unsupported);
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    let source = equip(&mut runtime, actor, &item);
    let unknown_actor = ActorId(u64::MAX);
    let unknown_item = ItemInstanceId(u64::MAX);
    assert_eq!(
        runtime.fire_resistance(unknown_actor),
        Err(Error::UnknownActor(unknown_actor))
    );
    assert_eq!(
        runtime.set_underlying_fire_resistance(unknown_actor, 20),
        Err(Error::UnknownActor(unknown_actor))
    );
    assert_eq!(
        runtime.activate_effect(unknown_item, actor, &effect),
        Err(Error::UnknownItem(unknown_item))
    );
    assert_eq!(
        runtime.activate_effect(source, unknown_actor, &effect),
        Err(Error::UnknownActor(unknown_actor))
    );
    assert_eq!(
        runtime.activate_effect(source, actor, &composite),
        Err(Error::UnsupportedOperation(unsupported))
    );
    assert_eq!(runtime.fire_resistance(actor), Ok(0));
    assert!(runtime.activations.is_empty());
}

#[test]
fn supported_operations_share_one_atomic_activation() {
    let item = item();
    let mut mixed = additive("smithing:effect/mixed", 1);
    mixed
        .operations
        .extend(fire_floor("arcanum:effect/53", 50).operations);
    mixed
        .operations
        .extend(fire_floor("smithing:effect/weaker", 40).operations);
    mixed
        .operations
        .extend(electrical("demo:effect/electrical", 8).operations);
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(i32::MAX).unwrap();
    let source = equip(&mut runtime, actor, &item);
    runtime.set_underlying_fire_resistance(actor, 20).unwrap();
    runtime
        .set_underlying_electrical_resistance(actor, 30)
        .unwrap();
    assert_eq!(
        runtime.activate_effect(source, actor, &mixed),
        Err(Error::ArmourClassOverflow)
    );
    assert_eq!(runtime.fire_resistance(actor), Ok(20));
    assert_eq!(runtime.electrical_resistance(actor), Ok(30));
    assert_eq!(runtime.item(source).unwrap().equipped_by, Some(actor));
    runtime.set_base_armour_class(actor, 10).unwrap();
    let id = runtime.activate_effect(source, actor, &mixed).unwrap();
    assert_eq!(runtime.armour_class(actor), Ok(11));
    assert_eq!(runtime.fire_resistance(actor), Ok(50));
    assert_eq!(runtime.electrical_resistance(actor), Ok(38));
    runtime.remove_activation(id).unwrap();
    assert_eq!(runtime.armour_class(actor), Ok(10));
    assert_eq!(runtime.fire_resistance(actor), Ok(20));
    assert_eq!(runtime.electrical_resistance(actor), Ok(30));
}

#[test]
fn out_of_range_changes_are_atomic_even_when_adjustments_cancel() {
    let item = item();
    let plus = additive("smithing:effect/plus", 1);
    let minus = additive("smithing:effect/minus", -1);
    let mut composite = additive("smithing:effect/composite", i32::MAX);
    composite
        .operations
        .push(EffectOperation::AddArmourClass { points: 1 });
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(i32::MAX).unwrap();
    let first = equip(&mut runtime, actor, &item);
    let second = equip(&mut runtime, actor, &item);
    assert_eq!(
        runtime.activate_effect(first, actor, &plus),
        Err(Error::ArmourClassOverflow)
    );
    let negative = runtime.activate_effect(first, actor, &minus).unwrap();
    assert_eq!(
        runtime.activate_effect(second, actor, &composite),
        Err(Error::ArmourClassOverflow)
    );
    let positive = runtime.activate_effect(second, actor, &plus).unwrap();
    assert_eq!(runtime.armour_class(actor), Ok(i32::MAX));
    assert_eq!(
        runtime.remove_activation(negative),
        Err(Error::ArmourClassOverflow)
    );
    assert_eq!(
        runtime.unequip(first, actor),
        Err(Error::ArmourClassOverflow)
    );
    assert_eq!(runtime.item(first).unwrap().equipped_by, Some(actor));
    assert_eq!(
        runtime.activation(negative).unwrap().armour_class_points,
        -1
    );
    runtime.remove_activation(positive).unwrap();
    assert_eq!(
        runtime.set_base_armour_class(actor, i32::MIN),
        Err(Error::ArmourClassOverflow)
    );
    assert_eq!(runtime.base_armour_class(actor), Ok(i32::MAX));
    runtime.unequip(first, actor).unwrap();
    assert_eq!(runtime.armour_class(actor), Ok(i32::MAX));
}

#[test]
fn same_inputs_produce_same_ids_and_signed_totals() {
    let item = item();
    let plus = additive("smithing:effect/plus", 4);
    let minus = additive("smithing:effect/minus", -3);
    let scenario = || {
        let mut runtime = Runtime::default();
        let actor = runtime.create_actor(10).unwrap();
        let source = equip(&mut runtime, actor, &item);
        let plus_id = runtime.activate_effect(source, actor, &plus).unwrap();
        let minus_id = runtime.activate_effect(source, actor, &minus).unwrap();
        let total = runtime.armour_class(actor).unwrap();
        runtime.unequip(source, actor).unwrap();
        (
            actor,
            source,
            plus_id,
            minus_id,
            total,
            runtime.armour_class(actor).unwrap(),
        )
    };
    let first = scenario();
    assert_eq!(first, scenario());
    assert_eq!((first.4, first.5), (11, 10));
}
