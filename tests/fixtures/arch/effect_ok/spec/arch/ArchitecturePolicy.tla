--------------------------- MODULE ArchitecturePolicy ---------------------------
EXTENDS FiniteSets
Modules == {
  "netty"
}
Owners == {
  "Bot"
}
Effects == {"env", "fs_write", "net", "process"}
Capabilities == {
  "holdout_read",
  "network_order_submit"
}
FloatAllowed == {}
AllowedDependencies == {}
AllowedStateWriters == {}
AllowedCapabilityCallers == {}
AllowedEffects == {
  <<"netty", "net">>
}
PathOwners == {}

WellFormed ==
  /\ \A dep \in AllowedDependencies : (
       dep[1] \in Modules /\ dep[2] \in Modules /\ dep[1] # dep[2])
  /\ \A w \in AllowedStateWriters : (
       w[1] \in Modules /\ Cardinality({ w2 \in AllowedStateWriters : w2[2] = w[2] }) = 1)
  /\ \A c \in AllowedCapabilityCallers : (
       c[1] \in Modules /\ c[2] \in Capabilities)
  /\ \A e \in AllowedEffects : (
       e[1] \in Modules /\ e[2] \in Effects)
  /\ FloatAllowed \subseteq Modules
  /\ \A p \in PathOwners : (p[2] \in Owners)
  /\ Effects = {"env", "fs_write", "net", "process"}


VARIABLE clock
Init == clock = 0
Next == (clock = 0 /\ clock' = 1) \/ (clock = 1 /\ clock' = 0)
Spec == Init /\ [][Next]_clock
=============================================================================
