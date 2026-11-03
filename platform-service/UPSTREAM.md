# Service source

Forked from clash-verge-service-ipc v2.7.6, GPL-3.0; see LICENSE. Suki uses its own service names, directories and protocol endpoints. It never adopts or removes the CV service. Legacy owner-file cleanup and old macOS helper uninstallation are removed. The application owns OS proxy intent and recovery. On macOS, the authenticated active service session executes the restricted proxy transaction engine and native adapter; it does not accept arbitrary commands or clear unrelated OS proxy settings. Current protocol revisions must match exactly; no old owner-format or service adoption is supported. Core watchdog retries are limited to three.

Revision 6 adds authenticated Windows network operations, following the ClashSuki native AppContainer approach. The helper accepts only known AppContainer identities and a fixed firewall operation for its approved core and the authenticated owner's application-local core. It never accepts arbitrary command lines or firewall program paths.

Revision 8 added session-scoped, bounded core-log history with a helper-start identity and monotonic cursor. The app and helper must be published together; older protocol revisions are rejected explicitly.

Current protocol revision 9 adds authenticated readback for a fixed Geo database allowlist, adapted from the CV dev update to service v2.7.7. Provider and Geo reads retain private-generation path restrictions; arbitrary copied assets and SSH key assets are not exposed. The app streams newer Geo data before local validation and after core Geo updates. SSH file-key staging is handled by the app runtime bundle. Package version 2.7.6 records the fork origin; it is not the current Suki protocol identity. App and helper must ship together; no older-protocol fallback is provided.
