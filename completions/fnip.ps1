# fnip: PowerShell completion
# Install with:  fnip --setup   (or manually: fnip --completions powershell | Out-String | Invoke-Expression)

Register-ArgumentCompleter -Native -CommandName fnip -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $elements = @($commandAst.CommandElements | ForEach-Object { $_.ToString() })
    # index of the word being completed (0 = "fnip").
    # If the user just typed a space, the new word isn't in the AST yet.
    $pos = if ($wordToComplete -eq '') { $elements.Count } else { $elements.Count - 1 }

    $emit = {
        param($items)
        $items | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
            [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
        }
    }

    $flags = '-a','-r','-l','-s','-d','-e','-x','-h',
             '--add','--remove','--list','--show','--dry','--exec','--help',
             '--setup','--unsetup','--install','--uninstall','--completions'

    if ($pos -eq 1) {
        & $emit ($flags + @(fnip --complete-names 2>$null))
        return
    }

    $first = $elements[1]
    if ($first -in '-a','--add','-l','--list','-h','--help','--setup','--unsetup','--install','--uninstall') { return }
    if ($first -eq '--completions') { & $emit @('bash','zsh','powershell'); return }

    $namePos = if ($first.StartsWith('-')) { 2 } else { 1 }

    if ($pos -eq $namePos) {
        & $emit @(fnip --complete-names 2>$null)
    }
    elseif ($pos -gt $namePos) {
        & $emit @(fnip --complete-vars $elements[$namePos] 2>$null)
    }
}
