#[tokio::test]
async fn natural_full_page_height_is_independent_of_900px_viewport() {
    let f = fixture();
    let image = image::RgbaImage::from_pixel(1440, 1200, image::Rgba([255, 255, 255, 255]));
    image.save(f.0.join("reference.png")).unwrap();
    image.save(f.0.join("actual.png")).unwrap();
    let mut i = input();
    i["environment"]["referenceResource"]["sha256"] =
        json!(hash(&std::fs::read(f.0.join("reference.png")).unwrap()));
    i["environment"]["viewport"] = json!({"width":1440,"height":900,"deviceScaleFactor":1});
    i["environment"]["captureContext"] = json!({"innerWidth":1440,"innerHeight":900,"devicePixelRatio":1,"visualViewportScale":1,"rootCssZoom":1,"bodyCssZoom":1});
    let result = run(i, &f.policy(), &ArtifactStore::default())
        .await
        .unwrap();
    assert_eq!(result["verdict"], "pass", "{result}");
    assert_eq!(
        result["environment"]["declaration"]["viewport"]["height"],
        900
    );
}

#[tokio::test]
async fn dpr_and_zoom_require_calibration_even_when_pixels_match() {
    let f = fixture();
    for (field, value) in [
        ("devicePixelRatio", 2),
        ("visualViewportScale", 2),
        ("rootCssZoom", 2),
        ("bodyCssZoom", 2),
    ] {
        let mut i = input();
        i["environment"]["captureContext"][field] = json!(value);
        if field == "devicePixelRatio" {
            i["environment"]["viewport"]["deviceScaleFactor"] = json!(value);
        }
        let result = run(i, &f.policy(), &ArtifactStore::default())
            .await
            .unwrap();
        assert_eq!(result["verdict"], "inconclusive", "{field}: {result}");
        assert_eq!(result["visual"]["passed"], true);
    }
}
