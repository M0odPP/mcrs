use mcrs_minecraft_protocol::{Decode, Packet};
use mcrs_minecraft_protocol_game::clientbound::{
    ClientboundRecipeBookAdd, ClientboundRecipeBookRemove, ClientboundRecipeBookSettings,
    ClientboundUpdateRecipes,
};
use mcrs_minecraft_protocol_game::recipe::RecipeBookType;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeRecipeBookType {
    Crafting,
    Furnace,
    BlastFurnace,
    Smoker,
}

impl From<ProbeRecipeBookType> for RecipeBookType {
    fn from(book: ProbeRecipeBookType) -> Self {
        match book {
            ProbeRecipeBookType::Crafting => Self::Crafting,
            ProbeRecipeBookType::Furnace => Self::Furnace,
            ProbeRecipeBookType::BlastFurnace => Self::BlastFurnace,
            ProbeRecipeBookType::Smoker => Self::Smoker,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeBookEvent {
    pub client: String,
    #[serde(flatten)]
    pub packet: RecipeBookPacket,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeSetSummary {
    pub name: String,
    pub item_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecipeBookPacket {
    UpdateRecipes {
        sets: Vec<RecipeSetSummary>,
        stonecutter: usize,
    },
    RecipeBookSettings {
        books: [[bool; 2]; 4],
    },
    RecipeBookAdd {
        replace: bool,
        count: usize,
        ids: Vec<i32>,
    },
    RecipeBookRemove {
        recipes: Vec<i32>,
    },
}

impl RecipeBookPacket {
    pub(crate) fn read(id: i32, mut body: &[u8]) -> anyhow::Result<Option<Self>> {
        if id == ClientboundUpdateRecipes::ID {
            let packet = ClientboundUpdateRecipes::decode(&mut body)?;
            let stonecutter = packet.stonecutter_recipes.len();
            let sets_iter = packet.item_sets.iter();
            let sets = sets_iter
                .map(|(key, list)| RecipeSetSummary {
                    name: key.as_str().to_owned(),
                    item_count: list.len(),
                })
                .collect();
            return Ok(Some(Self::UpdateRecipes { sets, stonecutter }));
        }
        if id == ClientboundRecipeBookSettings::ID {
            let raw = ClientboundRecipeBookSettings::decode(&mut body)?.book_settings;
            let books = [raw.crafting, raw.furnace, raw.blast_furnace, raw.smoker]
                .map(|book| [book.open, book.filtering]);
            return Ok(Some(Self::RecipeBookSettings { books }));
        }
        if id == ClientboundRecipeBookAdd::ID {
            let packet = ClientboundRecipeBookAdd::decode(&mut body)?;
            let (replace, count) = (packet.replace, packet.entries.len());
            let entries = packet.entries.iter().take(16);
            let ids = entries.map(|entry| entry.id.0).collect();
            return Ok(Some(Self::RecipeBookAdd {
                replace,
                count,
                ids,
            }));
        }
        if id == ClientboundRecipeBookRemove::ID {
            let packet = ClientboundRecipeBookRemove::decode(&mut body)?;
            let recipes = packet.recipes.into_iter().map(|item| item.0).collect();
            return Ok(Some(Self::RecipeBookRemove { recipes }));
        }
        Ok(None)
    }
}
