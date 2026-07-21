---
title: Updates
description: How PlotX checks for, downloads, and installs new versions.
---

PlotX keeps itself up to date. With automatic updates enabled (the default),
it periodically checks for new versions. Updates are downloaded and securely
verified in the background, so you can keep working. They are applied when
PlotX closes and take effect on the next launch.

## Checking and restarting

While an update downloads, its progress appears at the right end of the
toolbar. Once it is ready, select **Restart to update** to use the new version
immediately. PlotX asks about unsaved changes before restarting; canceling
leaves the app open and does not arm a later restart. If you close normally,
the update is installed after exit and takes effect next time you open PlotX.

**Preferences → General** shows the installed version and the updater's
state, offers **Check now** for an immediate manual check (also available as
**Check for updates…** in the command palette), and has its own
**Restart now** button.

Failed background checks (for example, when you are offline) are silent;
only a check you started yourself reports its error.

## Release channels

PlotX offers three release channels:

- **Stable** receives regular releases and is recommended for most users.
- **Beta** provides earlier access to features that are still being tested.
- **Alpha** provides the earliest builds and may be less reliable.

Choose a channel under **Update channel**. **Follow build** uses the channel
that came with your copy of PlotX. Changing to a more stable channel does not
downgrade the installed version; PlotX waits for a newer release on the
selected channel.

## Turning it off

Disable **Automatic updates** in **Preferences → General** to stop all
background checks. Manual checks with **Check now** still work.
