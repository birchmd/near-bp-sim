use crate::id::Id;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Role {
    BlockProducer,
    ChunkOnlyProducer,
    Delegator(Id),
}
