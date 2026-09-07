# Third-party notices

The syntax kinds, grammar productions, and indentation handling in `src/syntax/`
are adapted from gdscript-syntax in reactive-ui-toolkit/gdscript-analyzer.

Source: https://github.com/reactive-ui-toolkit/gdscript-analyzer
Revision: f5f70e1c35e1eff93658a4f3e8de01b889bbfee0
License selected: MIT. The original notice is in
[licenses/gdscript-syntax-MIT.txt](licenses/gdscript-syntax-MIT.txt).

Local adaptations replace the lexer and tree implementation, generate token
metadata and typed AST views with declarative macros, and modify parsing and
recovery behavior. Upstream code remains subject to its retained MIT notice.
