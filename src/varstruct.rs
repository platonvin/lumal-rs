// use paste::paste;

#[macro_export]
macro_rules! generate_varidic_struct {
    ($struct_type:ident, $struct_name:ident, $( $t:ty ),+ $(,)?) => {
        paste::paste! {
            // Generate the struct with one field per type
            #[allow(non_snake_case)]
            #[derive(Debug)]
            pub struct $struct_name {
                $( pub [<member_ $t>]: $struct_type<$t>, )+
            }

            // Implement an inherent method to access a member by type
            impl $struct_name {
                pub fn get<T>(&self) -> &$struct_type<T>
                where
                    Self: HasMember<T>,
                {
                    <Self as HasMember<T>>::get(self)
                }
            }
        }

        // Define a trait to associate a type with a member
        pub trait HasMember<T> {
            fn get(&self) -> &$struct_type<T>;
        }

        paste::paste! {
            // For each type in the list, implement the HasMember trait
            $(
                impl HasMember<$t> for $struct_name {
                    fn get(&self) -> &$struct_type<$t> {
                        &self.[<member_ $t>]
                    }
                }
            )+
        }
    };
}
