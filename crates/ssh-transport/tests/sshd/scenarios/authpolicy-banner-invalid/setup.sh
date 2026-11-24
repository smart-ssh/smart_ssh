#!/bin/sh
# 0xff 0xfe are not valid UTF-8.
printf "Authorized use \377\376 only: invalid banner bytes\n" > /etc/ssh/test-banner
