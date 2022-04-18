#[macro_export]
macro_rules! strings_vec {
    ($($x:expr),*) => (vec![$($x.to_string()),*]);
}
