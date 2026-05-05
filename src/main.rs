use hudhook::inject::Process;

fn main() {
    Process::by_title("METAL GEAR RISING: REVENGEANCE")
        .unwrap()
        .inject("drmod_rs_lib.dll".into())
        .unwrap();
}
