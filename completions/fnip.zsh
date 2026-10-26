#compdef fnip
# fnip: zsh completion
# Install with:  fnip --setup   (or manually: eval "$(fnip --completions zsh)")

# compdef only exists after compinit; if it hasn't run yet, run it now
(( $+functions[compdef] )) || { autoload -Uz compinit && compinit }

_fnip() {
    local -a flags names vars
    local first=$words[2] name pos

    flags=(
        '-a:add a snippet'         '--add:add a snippet'
        '-r:remove a snippet'      '--remove:remove a snippet'
        '-l:list snippets'         '--list:list snippets'
        '-s:show the template'     '--show:show the template'
        '-d:render without running' '--dry:render without running'
        '-e:run'                   '-x:run'  '--exec:run'
        '-h:help'                  '--help:help'
        '--setup:install completion' '--unsetup:remove completion'
        '--install:install fnip on PATH' '--uninstall:remove fnip from PATH'
        '--completions:print completion script'
    )
    names=("${(@f)$(fnip --complete-names 2>/dev/null)}")
    names=(${names:#})   # drop empty elements

    # first word: flags or names
    if (( CURRENT == 2 )); then
        _describe 'flag' flags
        _describe 'snippet' names
        return
    fi

    case $first in
        -a|--add|-l|--list|-h|--help|--setup|--unsetup|--install|--uninstall) return ;;
        --completions) compadd bash zsh powershell; return ;;
        -*) pos=3 ;;   # fnip -r <name>
        *)  pos=2 ;;   # fnip <name> var=...
    esac
    name=$words[$pos]

    if (( CURRENT == pos )); then
        _describe 'snippet' names
    elif (( CURRENT > pos )); then
        vars=("${(@f)$(fnip --complete-vars $name 2>/dev/null)}")
        vars=(${vars:#})
        compadd -S '' -a vars    # -S '' = no trailing space after "="
    fi
}

compdef _fnip fnip
