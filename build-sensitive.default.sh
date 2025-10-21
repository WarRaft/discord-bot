#!/bin/bash

# Project settings
# The name of your Rust project (should match the name in Cargo.toml)
PROJECT_NAME="discord-bot"

# Remote server SSH connection settings
# User account on the remote server (typically 'root' or your username)
REMOTE_USER="your_username"
# IP address or hostname of your remote server
REMOTE_HOST="your.server.ip.address"
# SSH port (default is 22)
REMOTE_PORT="22"
# Path on the remote server where the binary will be deployed
# This should match the ExecStart path in your systemd service file
REMOTE_PATH="/var/www/html/warraft/bin"
# SSH password (alternatively, you can use SSH keys for better security)
REMOTE_PASS="your_ssh_password"

# Service settings
# The name of your systemd service (without .service extension)
SERVICE_NAME="WarRaftDiscord"
# The name of the binary file on the remote server
# This should match the filename in the ExecStart path of your systemd service
BINARY_NAME="warraft-discord-linux"

# Discord bot token
# You can get this from Discord Developer Portal (https://discord.com/developers/applications)
# This should match the DISCORD_TOKEN in your systemd service Environment
DISCORD_TOKEN="your_discord_bot_token_here"

# Discord guild ID for instant command registration (optional)
# Right-click on server name in Discord (with Developer Mode enabled) and select "Copy Server ID"
# If set, slash commands will be registered to this specific guild (appears instantly)
# If empty, commands will be registered globally (can take up to 1 hour to propagate)
# This should match the DISCORD_GUILD_ID in your systemd service Environment
DISCORD_GUILD_ID=""

# MongoDB connection settings
# MongoDB connection URL with authentication
# Format: mongodb://username:password@host:port/authDatabase
# Example: mongodb://admin:password123@127.0.0.1:27017/admin
# This should match the MONGO_URL in your systemd service Environment
MONGO_URL="mongodb://admin:your_mongo_password@127.0.0.1:27017/admin"

# MongoDB database name
# The name of the database where bot state will be stored
# The bot will create a collection named 'discord_state' in this database
# This should match the MONGO_DB in your systemd service Environment
MONGO_DB="warraft"
