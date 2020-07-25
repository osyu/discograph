use serenity::client::Context;
use serenity::model::prelude::*;

use crate::cache::{Cache, CachedMessage, CachedUser};

#[derive(Debug)]
enum InteractionType {
    Message,
    Reaction,
}

#[derive(Debug)]
pub(crate) struct Interaction {
    what: InteractionType,
    source: UserId,
    targets: Vec<UserId>,
    channel: ChannelId,
    guild: GuildId,
}

impl Interaction {
    pub fn new_from_message(message: Message) -> Self {
        let user_mentions = message
            .mentions
            .iter()
            .map(|u| u.id)
            .collect::<Vec<UserId>>();

        Interaction {
            what: InteractionType::Message,
            source: message.author.id,
            targets: user_mentions,
            channel: message.channel_id,
            guild: message.guild_id.unwrap(),
        }
    }

    pub fn new_from_reaction(reaction: Reaction, target_message: CachedMessage) -> Self {
        Interaction {
            what: InteractionType::Reaction,
            source: reaction.user_id,
            targets: vec![target_message.author_id],
            channel: reaction.channel_id,
            guild: reaction.guild_id.unwrap(),
        }
    }

    pub fn to_string(&self, ctx: &Context, cache: &Cache) -> String {
        let user_to_display_name = |guild_id: GuildId, user: CachedUser| {
            format!(
                "\"{}\" ({}#{:04})",
                cache
                    .get_member(ctx, guild_id, user.id)
                    .unwrap()
                    .nick
                    .unwrap_or_else(|| user.name.clone()),
                user.name,
                user.discriminator
            )
        };

        let source_name = cache
            .get_user(&ctx, self.source)
            .map(|u| user_to_display_name(self.guild, u))
            .unwrap();

        let target_names = self
            .targets
            .iter()
            .map(|u| cache.get_user(&ctx, *u).unwrap())
            .map(|u| user_to_display_name(self.guild, u))
            .collect::<Vec<String>>()
            .join(", ");

        let channel_name = format!("#{}", cache.get_channel(&ctx, self.channel).unwrap().name);

        let guild_name = cache.get_guild(&ctx, self.guild).unwrap().name;

        match self.what {
            InteractionType::Message => format!(
                "new message from {} in {} @ \"{}\", mentions: [{}]",
                source_name, channel_name, guild_name, target_names
            ),
            InteractionType::Reaction => format!(
                "{} reacted to a message by {} in {} @ \"{}\"",
                source_name, target_names, channel_name, guild_name
            ),
        }
    }
}
