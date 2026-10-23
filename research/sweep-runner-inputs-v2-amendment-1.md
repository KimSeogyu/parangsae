# Amendment 1 to sweep-runner-inputs-v2

`research/sweep-runner-inputs-v2.md` stays frozen at sha256 `be7f5dd05d9adc9d557355e509a4ab1c33742599a123a0167812b0fbec01dc75`. This file does not change those bytes.

§2.3, the train-end sentence, is replaced by:

Train-window trades are flattened at the open of the last `count > 0` minute before the train end, and that flatten pays costs, so no train label uses test-period prices.

This replaces the frozen wording "closed at the train end (marked at the mark close)".
