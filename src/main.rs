#![allow(clippy::result_large_err)]

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::Client;
use inquire::{
    list_option::ListOption, validator::Validation, Confirm, CustomUserError, MultiSelect,
};
use std::process;

const MAX_BUCKETS: u8 = 5;
const PROTECTED_BUCKET_NAMES: &[&str] = &["backup", "do-not-delete", "console"];

#[tokio::main]
async fn main() {
    let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
    let config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&config);

    println!("Finding buckets...");

    let found_buckets = list_buckets(&client).await.unwrap_or_else(|err| {
        eprintln!("{}", err);
        process::exit(1)
    });

    let selected_buckets = MultiSelect::new("Select buckets to be removed", found_buckets)
        .with_validator(wrapper_validator)
        .prompt()
        .unwrap();

    println!("Deleting {} buckets", selected_buckets.len());
    println!("{}", selected_buckets.join("\n\t - "));

    let confirmation = Confirm::new(
        format!(
            "Do you wish to proceed? This action will delete {} buckets",
            selected_buckets.len()
        )
        .as_str(),
    )
    .with_default(false)
    .with_help_message("There's no turning back from here")
    .prompt()
    .unwrap_or(false);

    if !confirmation {
        println!("Quitting");
        process::exit(1);
    }

    for bucket in selected_buckets {
        println!("Deleting bucket: {}", bucket);
        empty_bucket(&client, &bucket).await.unwrap_or_else(|err| {
            eprintln!("Error emptying bucket {}: {}", bucket, err);
        });
        delete_bucket(&client, &bucket).await.unwrap_or_else(|err| {
            eprintln!("Error deleting bucket {}: {}", bucket, err);
        });
    }

    println!("Done! 💥")
}

async fn empty_bucket(client: &Client, name: &String) -> Result<(), aws_sdk_s3::Error> {
    let name = name.to_owned();
    let objects = client.list_object_versions().bucket(&name).send().await?;

    for object in objects.versions().unwrap_or_default() {
        client
            .delete_object()
            .bucket(&name)
            .key(object.key().unwrap_or_default())
            .version_id(object.version_id().unwrap_or_default())
            .send()
            .await?;
    }
    Ok(())
}

async fn delete_bucket(client: &Client, name: &String) -> Result<(), aws_sdk_s3::Error> {
    client.delete_bucket().bucket(name).send().await?;

    Ok(())
}

async fn list_buckets(client: &Client) -> Result<Vec<String>, aws_sdk_s3::Error> {
    let response = client.list_buckets().send().await?;

    let buckets = response.buckets().unwrap_or_default().to_vec();
    let ret: Vec<_> = buckets
        .iter()
        .map(|x| x.name().unwrap().to_owned())
        .collect();
    Ok(ret)
}

fn protect_names_validator(options: &[ListOption<&String>]) -> Result<Validation, CustomUserError> {
    let invalid = options.iter().any(|option| {
        PROTECTED_BUCKET_NAMES
            .iter()
            .any(|protected| option.value.contains(protected))
    });

    match invalid {
        false => Ok(Validation::Valid),
        true => Ok(Validation::Invalid(
            "Cannot delete buckets with protected names".into(),
        )),
    }
}

fn length_validator(options: &[ListOption<&String>]) -> Result<Validation, CustomUserError> {
    let length = options.len();
    if length > MAX_BUCKETS.into() {
        return Ok(Validation::Invalid(
            format!("Maximum of {} selections. You have {}", MAX_BUCKETS, length).into(),
        ));
    }

    if options.is_empty() {
        return Ok(Validation::Invalid("Must select a bucket".into()));
    }

    Ok(Validation::Valid)
}

fn wrapper_validator(options: &[ListOption<&String>]) -> Result<Validation, CustomUserError> {
    let validators = [protect_names_validator, length_validator];

    for validator in validators {
        if let Ok(Validation::Invalid(error)) = validator(options) {
            return Ok(Validation::Invalid(error));
        }
    }
    Ok(Validation::Valid)
}
