// PROTOTYPE. Throwaway probe for issue #20. Not harness code.
//
// sbx [--abi6] [--rw PATH]... [--ro PATH]... [--seccomp net,meta,signal] [--bench N] -- CMD [ARGS]...
//
// Applies Landlock using only ABI 3 rights (the ADR 4 floor) or, with --abi6, ABI 6, then an optional
// seccomp deny list, then execs CMD.
use landlock::{
    path_beneath_rules, Access, AccessFs, Ruleset, RulesetAttr, RulesetCreatedAttr, Scope, ABI,
};
use seccompiler::{apply_filter, BpfProgram, SeccompAction, SeccompFilter};
use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::Instant;

fn group(name: &str) -> Vec<i64> {
    match name {
        // io_uring can open sockets without the socket syscall.
        "net" => vec![
            libc::SYS_socket,
            libc::SYS_io_uring_setup,
            libc::SYS_io_uring_enter,
            libc::SYS_io_uring_register,
        ],
        "meta" => vec![
            libc::SYS_chmod,
            libc::SYS_fchmod,
            libc::SYS_fchmodat,
            452, // fchmodat2
            libc::SYS_chown,
            libc::SYS_lchown,
            libc::SYS_fchown,
            libc::SYS_fchownat,
            libc::SYS_utime,
            libc::SYS_utimes,
            libc::SYS_futimesat,
            libc::SYS_utimensat,
            libc::SYS_setxattr,
            libc::SYS_lsetxattr,
            libc::SYS_fsetxattr,
            libc::SYS_removexattr,
            libc::SYS_lremovexattr,
            libc::SYS_fremovexattr,
        ],
        "signal" => vec![
            libc::SYS_kill,
            libc::SYS_tkill,
            libc::SYS_tgkill,
            libc::SYS_rt_sigqueueinfo,
            libc::SYS_rt_tgsigqueueinfo,
            libc::SYS_pidfd_send_signal,
        ],
        other => panic!("unknown seccomp group {other}"),
    }
}

fn confine(rw: &[String], ro: &[String], seccomp: &[String], abi6: bool) {
    let abi = if abi6 { ABI::V6 } else { ABI::V3 };
    let mut ruleset = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))
        .unwrap();
    if abi6 {
        // Signal and abstract Unix socket scoping, new in ABI 6.
        ruleset = ruleset.scope(Scope::from_all(abi)).unwrap();
    }
    ruleset
        .create()
        .unwrap()
        .add_rules(path_beneath_rules(ro, AccessFs::from_read(abi)))
        .unwrap()
        .add_rules(path_beneath_rules(rw, AccessFs::from_all(abi)))
        .unwrap()
        .restrict_self()
        .unwrap();
    if seccomp.is_empty() {
        return;
    }
    let rules: BTreeMap<i64, Vec<_>> = seccomp
        .iter()
        .flat_map(|g| group(g))
        .map(|n| (n, vec![]))
        .collect();
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        std::env::consts::ARCH.try_into().unwrap(),
    )
    .unwrap();
    let prog: BpfProgram = filter.try_into().unwrap();
    apply_filter(&prog).unwrap();
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut rw, mut ro, mut seccomp, mut bench, mut abi6) = (vec![], vec![], vec![], 0u32, false);
    let mut cmd = vec![];
    while let Some(a) = args.next() {
        match a.as_str() {
            "--rw" => rw.push(args.next().unwrap()),
            "--ro" => ro.push(args.next().unwrap()),
            "--seccomp" => {
                seccomp = args.next().unwrap().split(',').map(String::from).collect()
            }
            "--abi6" => abi6 = true,
            "--bench" => bench = args.next().unwrap().parse().unwrap(),
            "--" => {
                cmd = args.by_ref().collect();
            }
            other => panic!("unknown flag {other}"),
        }
    }
    if bench > 0 {
        // Spawn CMD `bench` times bare, then `bench` times confined in the child.
        for confined in [false, true] {
            let start = Instant::now();
            for _ in 0..bench {
                let mut c = Command::new(&cmd[0]);
                c.args(&cmd[1..]);
                if confined {
                    let (rw, ro, sc) = (rw.clone(), ro.clone(), seccomp.clone());
                    unsafe {
                        c.pre_exec(move || {
                            confine(&rw, &ro, &sc, abi6);
                            Ok(())
                        });
                    }
                }
                c.status().unwrap();
            }
            let per = start.elapsed().as_secs_f64() * 1e3 / bench as f64;
            println!("{}: {per:.3} ms per spawn", if confined { "confined" } else { "bare" });
        }
        return;
    }
    confine(&rw, &ro, &seccomp, abi6);
    let err = Command::new(&cmd[0]).args(&cmd[1..]).exec();
    eprintln!("exec failed: {err}");
    std::process::exit(127);
}
