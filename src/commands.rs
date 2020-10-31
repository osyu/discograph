use anyhow::{Context as AnyhowContext, Result};
use futures::future::join_all;
use tokio::io::AsyncWriteExt;
use tokio::process;
use tracing::{debug, error, info};
use twilight_command_parser::{Arguments, CommandParserConfig, Parser};
use twilight_model::channel::embed::{Embed, EmbedField, EmbedFooter};
use twilight_model::channel::Message;
use twilight_model::gateway::event::Event;
use twilight_model::gateway::event::Event::MessageCreate;
use twilight_model::id::GuildId;

use std::process::Stdio;

use crate::context::Context;

pub async fn handle_event(context: &Context, event: &Event) -> Result<bool> {
    match event {
        MessageCreate(message) => handle_message(context, message).await,
        _ => Ok(false),
    }
}

async fn handle_message(context: &Context, message: &Message) -> Result<bool> {
    // Ignore messages from bots (including ourself)
    if message.author.bot {
        return Ok(false);
    }

    debug!("new message: {}", message.content);

    // TODO: I think we want to switch back to our own command parsing.
    let mut config = CommandParserConfig::new();
    config.add_prefix(format!("<@{}> ", context.user.id));
    config.add_prefix(format!("<@!{}> ", context.user.id));
    config.add_command("help", false);
    config.add_command("invite", false);
    config.add_command("graph", false);
    config.add_command("stats", false);
    config.add_command("dump", false);

    let parser = Parser::new(config);
    let command = match parser.parse(&message.content) {
        Some(command) => command,
        None => return Ok(false),
    };

    info!("received command: {:?} in message {:?}", command, message);

    let result = match command.name {
        "help" | "invite" => command_help(context, message).await,
        "graph" => command_graph(context, message).await,
        "stats" => command_stats(context, message).await,
        "dump" => command_dump(context, message, command.arguments).await,
        _ => Ok(()),
    };

    if let Err(error) = result {
        error!("command failed: {}", error);

        context
            .http
            .create_message(message.channel_id)
            .content("Sorry, there was an error handling that command")?
            .await?;
    }

    Ok(true)
}

async fn command_help(context: &Context, message: &Message) -> Result<()> {
    let description = format!(
        "I'm a Discord Bot that infers relationships between users and draws pretty graphs.\n\
        I'll only respond to messages that directly mention me, like `@{} help`.",
        context.user.name,
    );

    let commands_field = EmbedField {
        inline: false,
        name: "Commands".to_string(),
        value: vec![
            "` help  `\u{2000}This message.",
            "` graph `\u{2000}Get a preview-quality graph image.",
        ]
        .join("\n"),
    };

    let invite_url = format!(
        "https://discord.com/api/oauth2/authorize?client_id={}&permissions=117824&scope=bot",
        context.user.id,
    );

    let invite_field = EmbedField {
        inline: false,
        name: "Want graphs for your guild?".to_string(),
        value: format!(
            "[Click here]({}) to invite the bot to join your server.",
            invite_url,
        ),
    };

    let footer = EmbedFooter {
        icon_url: None,
        proxy_icon_url: None,
        text: format!(
            "Sent in response to a command from {}#{:04}",
            message.author.name, message.author.discriminator,
        ),
    };

    let embed = Embed {
        author: None,
        color: None,
        description: Some(description),
        fields: vec![commands_field, invite_field],
        footer: Some(footer),
        image: None,
        kind: "rich".to_string(),
        provider: None,
        thumbnail: None,
        timestamp: None,
        title: None,
        url: None,
        video: None,
    };

    context
        .http
        .create_message(message.channel_id)
        .embed(embed)?
        .await?;

    Ok(())
}

async fn command_graph(context: &Context, message: &Message) -> Result<()> {
    // TODO: Respond to the command on errors.

    let guild_id = message.guild_id.context("message not to guild")?;
    let guild_name = context.cache.get_guild(guild_id).await?.name;

    let graph = {
        let social = context.social.lock();

        social
            .build_guild_graph(guild_id)
            .context("no graph for guild")?
    };

    let dot = graph
        .to_dot(context, guild_id, Some(&message.author))
        .await?;

    let png = render_dot(&dot).await?;

    context
        .http
        .create_message(message.channel_id)
        .attachment(format!("{}.png", guild_name), png)
        .await?;

    Ok(())
}

async fn command_stats(context: &Context, message: &Message) -> Result<()> {
    context
        .http
        .create_message(message.channel_id)
        .content(format!("{:?}", context.cache.get_stats()))?
        .await?;

    Ok(())
}

async fn command_dump(
    context: &Context,
    message: &Message,
    mut arguments: Arguments<'_>,
) -> Result<()> {
    if !context.owners.contains(&message.author.id) {
        info!(
            "{} tried to run dump command but isn't an owner",
            message.author.id,
        );
        return Ok(());
    }

    if let Some(guild_id) = arguments.next() {
        let guild_id: u64 = guild_id.parse()?;
        let guild_id = GuildId(guild_id);

        let guild_name = context.cache.get_guild(guild_id).await?.name;

        let graph = {
            let social = context.social.lock();

            social
                .build_guild_graph(guild_id)
                .context("no graph for guild")?
        };

        let dot = graph.to_dot(context, guild_id, None).await?;

        let png = render_dot(&dot).await?;

        context
            .http
            .create_message(message.channel_id)
            .attachment(format!("{}.dot", guild_name), dot)
            .attachment(format!("{}.png", guild_name), png)
            .await?;

        return Ok(());
    }

    let guild_ids = {
        let social = context.social.lock();
        social.get_all_guild_ids()
    };

    let guild_futures = guild_ids
        .into_iter()
        .map(|guild_id| context.cache.get_guild(guild_id));

    let guilds: Vec<_> = join_all(guild_futures)
        .await
        .into_iter()
        .filter_map(|guild| guild.ok())
        .map(|guild| format!("{} - {}", guild.id, guild.name))
        .collect();

    let mut content = "Guilds:\n".to_owned();
    content.push_str(&guilds.join("\n"));

    context
        .http
        .create_message(message.channel_id)
        .content(content)?
        .await?;

    Ok(())
}

async fn render_dot(dot: &str) -> Result<Vec<u8>> {
    let mut graphviz = process::Command::new("dot")
        .arg("-v")
        .arg("-Tpng")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    {
        let stdin = graphviz.stdin.as_mut().unwrap();
        stdin.write_all(dot.as_bytes()).await?;
    }

    let output = graphviz.wait_with_output().await?;

    if !output.status.success() {
        anyhow::bail!("graphviz failed");
    }

    Ok(output.stdout)
}
