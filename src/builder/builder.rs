pub trait Builder<'w: 'a, 's: 'a, 'a> : Sized {
    fn get_commands(&mut self) -> &mut EntityCommands<'w, 's, 'a>;
}