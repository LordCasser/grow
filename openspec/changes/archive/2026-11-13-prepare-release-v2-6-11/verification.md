## Release correction

The v2.6.10 Core job failed to compile Pager with E0658 in the paged copy match guard. The replacement uses an ordinary conditional with stable let chains. The existing paged copy regression remains in the Pager suite run by the official release gate.
