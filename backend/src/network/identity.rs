use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Network {
    Mainnet,
    Testnet,
    Futurenet,
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Network::Mainnet => write!(f, "mainnet"),
            Network::Testnet => write!(f, "testnet"),
            Network::Futurenet => write!(f, "futurenet"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableSchema {
    pub name: String,
    pub fields: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum NetworkSchemaError {
    #[error("schema missing network discriminator: {0}")]
    MissingNetworkDiscriminator(String),
}

pub struct NetworkSchema;

impl NetworkSchema {
    pub fn validate(schemas: &[TableSchema]) -> Result<(), String> {
        for schema in schemas {
            let has_network = schema
                .fields
                .iter()
                .any(|field| field.eq_ignore_ascii_case("network"));
            if !has_network {
                return Err(format!(
                    "schema missing network discriminator for table {}",
                    schema.name
                ));
            }
        }

        Ok(())
    }

    pub fn assert_startup_schema(schemas: &[TableSchema]) {
        match Self::validate(schemas) {
            Ok(()) => {}
            Err(message) => panic!("startup schema check failed: {message}"),
        }
    }
}
