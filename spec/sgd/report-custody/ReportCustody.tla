--------------------------- MODULE ReportCustody ---------------------------
CONSTANT InitiallyReady
VARIABLES phase, ready, resident
vars == <<phase, ready, resident>>

Init == /\ phase = "Ready"
        /\ ready = InitiallyReady
        /\ resident = TRUE

Prepare == /\ phase = "Ready" /\ ~ready
           /\ ready' = TRUE
           /\ UNCHANGED <<phase, resident>>
StartReport == /\ phase = "Ready" /\ ready
               /\ phase' = "Reporting"
               /\ UNCHANGED <<ready, resident>>
ReportSucceeded == /\ phase = "Reporting"
                   /\ phase' = "Succeeded"
                   /\ UNCHANGED <<ready, resident>>
ReportFailed == /\ phase = "Reporting"
                /\ phase' = "Failed"
                /\ UNCHANGED <<ready, resident>>
Wait == /\ phase \in {"Succeeded", "Failed"}
        /\ UNCHANGED vars
Next == Prepare \/ StartReport \/ ReportSucceeded \/ ReportFailed \/ Wait
Spec == Init /\ [][Next]_vars
PayloadRetained == resident = TRUE
TypeOK == /\ phase \in {"Ready", "Reporting", "Succeeded", "Failed"}
          /\ ready \in BOOLEAN /\ resident \in BOOLEAN
=============================================================================
