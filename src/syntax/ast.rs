use super::{Node, SyntaxKind as K, Token};

pub trait AstNode<'a>: Sized {
    fn cast(node: Node<'a>) -> Option<Self>;
    fn syntax(&self) -> Node<'a>;
}

macro_rules! ast_nodes {
    ($( $name:ident => $($kind:ident)|+ ),* $(,)?) => {
        $(
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            pub struct $name<'a>(Node<'a>);

            impl<'a> AstNode<'a> for $name<'a> {
                fn cast(node: Node<'a>) -> Option<Self> {
                    matches!(node.kind(), $(K::$kind)|+).then_some(Self(node))
                }

                fn syntax(&self) -> Node<'a> { self.0 }
            }
        )*
    };
}

ast_nodes! {
    SourceFile => SourceFile,
    Function => FuncDecl,
    Variable => VarDecl,
    Constant => ConstDecl,
    Class => InnerClassDecl,
    Block => Block,
    If => IfStmt,
    Return => ReturnStmt,
    Parameter => Param | VarargParam,
    Type => TypeRef,
    Binary => BinExpr,
    Assignment => AssignExpr,
    Call => CallExpr,
    Lambda => LambdaExpr,
    Annotation => Annotation,
}

pub fn children<'a, T: AstNode<'a> + 'a>(node: Node<'a>) -> impl Iterator<Item = T> + 'a {
    node.children().filter_map(T::cast)
}

impl<'a> Function<'a> {
    pub fn name(self) -> Option<&'a str> {
        let name = self.0.children().find(|node| node.kind() == K::Name)?;
        let token = name.tokens().find(|token| !token.kind.is_trivia())?;
        let relative = token.range.start - name.range().start;
        Some(&name.text()[relative..relative + token.range.end - token.range.start])
    }

    pub fn body(self) -> Option<Block<'a>> {
        children(self.0).next()
    }

    pub fn parameters(self) -> impl Iterator<Item = Parameter<'a>> + 'a {
        self.0
            .children()
            .filter(|node| node.kind() == K::ParamList)
            .flat_map(children)
    }
}

impl<'a> Binary<'a> {
    pub fn operands(self) -> impl Iterator<Item = Node<'a>> + 'a {
        self.0.children()
    }

    pub fn operator(self) -> Option<&'a Token> {
        self.0
            .children_with_tokens()
            .find_map(|element| match element {
                super::Element::Token(token)
                    if !token.kind.is_trivia() && !token.kind.is_synthetic_layout() =>
                {
                    Some(token)
                }
                _ => None,
            })
    }
}

impl Parameter<'_> {
    pub fn is_variadic(self) -> bool {
        self.0.kind() == K::VarargParam
    }
}

macro_rules! ast_enum {
    ($name:ident { $($variant:ident => $kind:ident),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name<'a> { $($variant(Node<'a>)),* }

        impl<'a> AstNode<'a> for $name<'a> {
            fn cast(node: Node<'a>) -> Option<Self> {
                match node.kind() {
                    $(K::$kind => Some(Self::$variant(node)),)*
                    _ => None,
                }
            }

            fn syntax(&self) -> Node<'a> {
                match self { $(Self::$variant(node) => *node),* }
            }
        }
    };
}

ast_enum! {
    Expression {
        Binary => BinExpr,
        Unary => UnaryExpr,
        Ternary => TernaryExpr,
        Cast => CastExpr,
        TypeTest => IsExpr,
        Membership => InExpr,
        Call => CallExpr,
        Index => IndexExpr,
        Field => FieldExpr,
        Await => AwaitExpr,
        Lambda => LambdaExpr,
        Parenthesized => ParenExpr,
        Array => ArrayLit,
        Dictionary => DictLit,
        Name => NameRef,
        Literal => Literal,
        NodePath => GetNodeExpr,
        UniqueNode => UniqueNodeExpr,
        Preload => PreloadExpr,
    }
}
