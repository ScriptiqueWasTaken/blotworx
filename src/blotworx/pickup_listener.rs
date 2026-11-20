//! Native pickup listener routing, REL38a2cc/38a368. Physics registration and
//! broadphase event generation remain outside this synchronous callback path.
use super::{assets::RuntimeEntity, collidable_owner, phantom_overlaps, pickup::Receiver};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
pub trait Bindings: collidable_owner::Records {
    /// Visiting body userdata belongs to the physics world's entity registry.
    fn entity_identity(&self, userdata: usize) -> Result<Option<usize>>;
}
pub fn deliver(
    owner: usize,
    visitor: usize,
    entered: bool,
    entities: &[RuntimeEntity],
    receivers: &mut BTreeMap<usize, Receiver>,
    records: &impl collidable_owner::Records,
    resolve: impl Fn(usize) -> Result<Option<usize>>,
) -> Result<()> {
    let owner = collidable_owner::from_owner(owner, records)?
        .context("Pickup phantom has no entity userdata")?;
    let owner = resolve(owner)?.context("Pickup phantom entity is not live")?;
    deliver_entity(
        owner, visitor, entered, entities, receivers, records, resolve,
    )
}
fn deliver_entity(
    owner: usize,
    visitor: usize,
    entered: bool,
    entities: &[RuntimeEntity],
    receivers: &mut BTreeMap<usize, Receiver>,
    records: &impl collidable_owner::Records,
    resolve: impl Fn(usize) -> Result<Option<usize>>,
) -> Result<()> {
    let receiver = receivers
        .get_mut(&owner)
        .context("Phantom owner has no pickup receiver")?;
    let exists = |index| index < entities.len();
    let occupied = receiver.collector(exists).is_some();
    if entered == occupied {
        return Ok(());
    }
    let other = collidable_owner::from_collidable(visitor, records)?
        .map(resolve)
        .transpose()?
        .flatten();
    ensure!(
        other.is_none_or(|index| index < entities.len()),
        "Unknown overlap entity"
    );
    if entered {
        receiver.enter(other, exists, |index| {
            entities[index]
                .components
                .iter()
                .any(|c| c.class_name == "InventoryComponent")
        });
    } else {
        receiver.exit(other, exists);
    }
    Ok(())
}
pub struct Callbacks<'a, B> {
    pub entities: &'a [RuntimeEntity],
    pub receivers: &'a mut BTreeMap<usize, Receiver>,
    pub listeners: &'a BTreeMap<usize, super::hk_reference::Header>,
    pub bindings: &'a B,
    pub phantom: usize,
    pub user_data: &'a Option<collidable_owner::EntityData>,
}
impl<B: Bindings> Callbacks<'_, B> {
    fn event(
        &mut self,
        listener: usize,
        phantom: Option<usize>,
        visitor: usize,
        entered: bool,
    ) -> Result<()> {
        ensure!(
            self.listeners.contains_key(&listener),
            "Unknown pickup listener identity"
        );
        ensure!(
            phantom == Some(self.phantom),
            "Pickup overlap owning phantom identity changed"
        );
        let token = match self
            .user_data
            .map(|data| data.owner())
            .unwrap_or(collidable_owner::UserData::Missing)
        {
            collidable_owner::UserData::Entity(token) => token,
            collidable_owner::UserData::Missing => {
                anyhow::bail!("Pickup phantom has no owned entity userdata")
            }
            collidable_owner::UserData::Embedded => anyhow::bail!(
                "Owned embedded phantom userdata requires its reconstructed collidable records"
            ),
        };
        let owner = token
            .checked_sub(1)
            .filter(|index| *index < self.entities.len())
            .context("Pickup phantom entity token is not live")?;
        deliver_entity(
            owner,
            visitor,
            entered,
            self.entities,
            self.receivers,
            self.bindings,
            |p| self.bindings.entity_identity(p),
        )
    }
}
impl<B: Bindings> phantom_overlaps::Callbacks for Callbacks<'_, B> {
    fn added(
        &mut self,
        l: usize,
        _: &mut phantom_overlaps::Overlaps,
        e: &mut phantom_overlaps::Added,
    ) -> Result<()> {
        self.event(l, e.phantom, e.collidable, true)
    }
    fn removed(
        &mut self,
        l: usize,
        _: &mut phantom_overlaps::Overlaps,
        e: &mut phantom_overlaps::Removed,
    ) -> Result<()> {
        self.event(l, e.phantom, e.collidable, false)
    }
}
