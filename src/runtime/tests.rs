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
    let floor = EffectOperation::ResistanceFloor {
        channel: ResistanceKind::Fire,
        minimum_percent: 50,
    };
    effect.operations.push(floor);
    let mut runtime = Runtime::default();
    let actor = runtime.create_actor(10).unwrap();
    let source = equip(&mut runtime, actor, &item);
    assert_eq!(
        runtime.activate_effect(source, actor, &effect),
        Err(Error::UnsupportedOperation(floor))
    );
    assert_eq!(runtime.armour_class(actor), Ok(10));
    assert!(runtime.activations.is_empty());
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
