#!/usr/bin/env bash

sudo ip netns delete client-ns
sudo ip netns delete server-ns
sudo ip netns delete forward-ns
