#!/usr/bin/env python3
"""CLI for Microsoft 365 APIs via MSAL tokens from Chrome."""
import json
import sys
import click

from lib.commands.auth import auth
from lib.commands.graph import graph_cmd
from lib.commands.rest import rest_cmd
from lib.commands.me import me_cmd
from lib.commands.chats import chats_cmd
from lib.commands.meetings import meetings_cmd
from lib.commands.search import search_cmd
from lib.commands.people import people_cmd
from lib.commands.files import files_cmd
from lib.commands.mail import mail_cmd
from lib.commands.calendar import calendar_cmd
from lib.commands.loop import loop_cmd
from lib.commands.channels import channels_cmd
from lib.commands.tag import tag_cmd


def out(obj):
    sys.stdout.write(json.dumps(obj, indent=2, default=str) + '\n')


def text(s):
    sys.stdout.write(str(s) + '\n')


def die(error):
    out({'ok': False, 'error': str(error)})
    sys.exit(1)


@click.group()
@click.version_option('0.1.0')
@click.pass_context
def cli(ctx):
    """CLI for Microsoft 365 APIs via MSAL tokens from Chrome."""
    ctx.ensure_object(dict)
    ctx.obj['out'] = out
    ctx.obj['text'] = text
    ctx.obj['die'] = die


cli.add_command(auth)
cli.add_command(graph_cmd)
cli.add_command(rest_cmd)
cli.add_command(me_cmd)
cli.add_command(chats_cmd)
cli.add_command(meetings_cmd)
cli.add_command(search_cmd)
cli.add_command(people_cmd)
cli.add_command(files_cmd)
cli.add_command(mail_cmd)
cli.add_command(calendar_cmd)
cli.add_command(loop_cmd)
cli.add_command(channels_cmd)
cli.add_command(tag_cmd)

if __name__ == '__main__':
    cli()
