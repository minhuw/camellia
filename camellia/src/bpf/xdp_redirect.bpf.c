#include "vmlinux.h"
#include <bpf/bpf_helpers.h>

SEC("xdp")
int xdp_redirect(struct xdp_md *ctx)
{
	return XDP_REDIRECT;
}

char __license[] SEC("license") = "GPL";
