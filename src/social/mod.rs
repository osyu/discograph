pub mod graph;
pub mod inference;

use anyhow::Result;
use tracing::info;
use twilight_model::channel::message::MessageType;
use twilight_model::channel::{Channel, ChannelType, GuildChannel};
use twilight_model::gateway::event::Event;
use twilight_model::gateway::event::Event::{
    ChannelCreate, GuildCreate, MessageCreate, ReactionAdd,
};
use twilight_model::gateway::payload::ChannelCreate as ChannelCreatePayload;

use crate::context::Context;
use crate::social::inference::Interaction;

pub async fn handle_event(context: &Context, event: &Event) -> Result<()> {
    match event {
        GuildCreate(guild) => {
            // Load any existing graphs into memory for the guild's channels.
            let mut social = context.social.lock();
            for &channel_id in guild.channels.keys() {
                social.get_graph(guild.id, channel_id);
            }
        }
        ChannelCreate(ChannelCreatePayload(Channel::Guild(GuildChannel::Text(channel))))
            if channel.kind == ChannelType::GuildText =>
        {
            if let Some(guild_id) = channel.guild_id {
                // Load any existing graph into memory for the channel.
                let mut social = context.social.lock();
                social.get_graph(guild_id, channel.id);
            }
        }
        MessageCreate(message) if message.kind == MessageType::Regular => {
            // Ignore messages from ourselves
            if message.author.id != context.user.id {
                let interaction = Interaction::new_from_message(message)?;
                process_interaction(context, interaction).await;
            }
        }
        ReactionAdd(reaction) => {
            // Ignore reactions from ourselves
            if reaction.user_id != context.user.id {
                let message = context
                    .cache
                    .get_message(reaction.channel_id, reaction.message_id)
                    .await?;

                let interaction = Interaction::new_from_reaction(reaction, &message)?;
                process_interaction(context, interaction).await;
            }
        }
        _ => (),
    }

    Ok(())
}

async fn process_interaction(context: &Context, interaction: Interaction) {
    let interaction_string = interaction.to_string(&context.cache).await;
    info!("{}", interaction_string);

    let mut social = context.social.lock();

    let changes = social.infer(&interaction);
    for change in &changes {
        info!("-> {:?}", change);
    }

    social.apply(&interaction, &changes);
}