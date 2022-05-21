# Security Policy

This is a fun, small scale project, but I do take security seriously, but it also has to be related to scope and purpuse of this project.

This project is definitelly **not** critical infrastructure and is intended to be used as such - for personal sharing of audio files, with low value 
and low impact if something goes wrong.

For security considerations and recommendations please also check [Security section in README](https://github.com/izderadicka/audioserve#security).

## Supported Versions

**Definitelly do not use versions older that v15.0.0,  these had significant security vulnerability - confidentiality problem related to path relative traversal 
[CWE-23](https://cwe.mitre.org/data/definitions/23.html) type of vulnerability.**

Considering my limited  capacity, I'm including security patches to `master` branch on best effort basis, and releasing new versions only for critical security fixes 
(plus irregularly there are new releases with new functionality). 

For such small project I do not expect any CVEs or similar.  If there will be really critical security problem I'll put also notice on top of README file.


## Reporting a Vulnerability

Just file an issue on github page of this project.

I do not see significant issue with sharing, I doubt such small project will make sense for 0-day attacks.
You can also let me know on my email, or note in an issue that you want to use other channel for details.

I try to respond to issues as soon as I can.
