# fnip: bash completion
# Install with:  fnip --setup   (or manually: eval "$(fnip --completions bash)")

_fnip_complete() {
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local flags="-a -r -l -s -d -e -x -h -p --add --remove --list --show --dry --exec --help --setup --unsetup --install --uninstall --completions --json --prompt"
    local first="${COMP_WORDS[1]}"
    local name pos

    # bash treats '=' as a word separator (COMP_WORDBREAKS):
    # "ver=18" arrives as 3 words: ver, =, 18. We don't complete values.
    [[ "$cur" == "=" ]] && return
    [[ "${COMP_WORDS[COMP_CWORD-1]}" == "=" ]] && return

    # first word: flags or snippet names
    if (( COMP_CWORD == 1 )); then
        COMPREPLY=( $(compgen -W "$flags $(fnip --complete-names 2>/dev/null)" -- "$cur") )
        return
    fi

    case "$first" in
        -a|--add) COMPREPLY=( $(compgen -W "-p --prompt" -- "$cur") ); return ;;
        -l|--list|-h|--help|--setup|--unsetup|--install|--uninstall) return ;;   # nothing to complete after these
        --completions) COMPREPLY=( $(compgen -W "bash zsh powershell" -- "$cur") ); return ;;
        -*) pos=2 ;;                              # fnip -r <name>
        *)  pos=1 ;;                              # fnip <name> var=...
    esac
    name="${COMP_WORDS[pos]}"

    if (( COMP_CWORD == pos )); then
        COMPREPLY=( $(compgen -W "$(fnip --complete-names 2>/dev/null)" -- "$cur") )
    elif (( COMP_CWORD > pos )); then
        # suggest "var=" for the template's variables
        COMPREPLY=( $(compgen -W "$(fnip --complete-vars "$name" 2>/dev/null)" -- "$cur") )
        compopt -o nospace 2>/dev/null            # no trailing space after "="
    fi
}

complete -F _fnip_complete fnip
