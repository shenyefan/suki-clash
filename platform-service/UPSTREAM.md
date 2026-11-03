# Service source

Forked from clash-verge-service-ipc v2.7.6, GPL-3.0; see LICENSE. Suki uses its own service names, directories and protocol endpoints. It never adopts or removes the CV service. Legacy owner-file cleanup and old macOS helper uninstallation are removed. The application owns OS proxy intent and recovery. On macOS, the authenticated active service session executes the restricted proxy transaction engine and native adapter; it does not accept arbitrary commands or clear unrelated OS proxy settings. Current protocol revisions must match exactly; no old owner-format or service adoption is supported. Core watchdog retries are limited to three.

Revision 6 adds authenticated Windows network operations, following the ClashSuki native AppContainer approach. The helper accepts only known AppContainer identities and a fixed firewall operation for its approved core and the authenticated owner's application-local core. It never accepts arbitrary command lines or firewall program paths.

Current protocol revision 8 includes session-scoped, bounded core-log history with a helper-start identity and monotonic cursor. The app and helper must be published together; older protocol revisions are rejected explicitly.
