#!/bin/env bash

sudo ip netns del client-ns
sudo ip netns del server-ns 
sudo ip netns del forward-ns