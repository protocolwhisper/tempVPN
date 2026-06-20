on run argv
    if (count of argv) is not 3 then error "Expected client, session response, and private key paths"

    set clientPath to item 1 of argv
    set sessionPath to item 2 of argv
    set privateKeyPath to item 3 of argv
    set connectCommand to quoted form of clientPath & " connect --session-response " & quoted form of sessionPath & " --private-key-path " & quoted form of privateKeyPath

    set commandOutput to do shell script connectCommand with administrator privileges
    return commandOutput
end run
