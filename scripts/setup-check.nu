def check-setting [key: string, expected: string] {
    let local_value = (try { git config --local --get $key } catch { "<unset>" })
    let effective_value = (try { git config --get $key } catch { "<unset>" })
    if $local_value != $expected or $effective_value != $expected {
        error make --unspanned $"FAIL  ($key) expected ($expected), local ($local_value), effective ($effective_value)"
    }
    print $"OK  ($key) = ($effective_value)"
}

check-setting core.hooksPath .githooks
check-setting merge.ff "false"
let hook = (git rev-parse --git-path hooks/pre-commit)
if ($hook | path expand | path type) != file or (test -x $hook | complete).exit_code != 0 {
    print --stderr $"FAIL  Expected an executable pre-commit hook at ($hook)"
    exit 1
}
print $"OK  ($hook) is executable"
