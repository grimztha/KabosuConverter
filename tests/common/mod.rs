pub fn init_test_backend() {
    let _ = slint::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: false,
                threading: true,
            },
        ),
    ));
}
