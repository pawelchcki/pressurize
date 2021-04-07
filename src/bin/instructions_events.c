#include <linux/ptrace.h>
#include <uapi/linux/bpf_perf_event.h>

struct key_t {
    int cpu;
    int pid;
    char name[TASK_COMM_LEN];
};

BPF_HASH(instruction_count, struct key_t);

static inline __attribute__((always_inline)) void get_key(struct key_t* key) {
    key->pid = bpf_get_current_pid_tgid();
    key->cpu = bpf_get_smp_processor_id();
    bpf_get_current_comm(&(key->name), sizeof(key->name));
}

int on_instructions(struct bpf_perf_event_data *ctx) {
    struct key_t key = {};

    get_key(&key);
    struct bpf_perf_event_value val;
    if (key.pid == 0){
        return 0;
    }
	long err = bpf_perf_prog_read_value(ctx, &val, sizeof(val));
	if (err){
		return 0;
    }
    instruction_count.update(&key, &val.counter);
    return 0;
}
