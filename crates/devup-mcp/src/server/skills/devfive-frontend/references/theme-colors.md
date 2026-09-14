# Theme Colors Reference

데브파이브 프로젝트들은 `devup.json` 파일에서 테마 색상을 정의합니다. 각 프로젝트마다 브랜드 색상은 다르지만, **동일한 토큰 구조**를 사용합니다.

## 공통 토큰 구조

모든 프로젝트에서 다음 토큰들을 사용할 수 있습니다:

### Primary Colors (프로젝트별 상이)

| Token | Description | Usage |
|-------|-------------|-------|
| `$primary` | 메인 브랜드 색상 | 주요 버튼, 강조 요소 |
| `$primaryBold` | Primary hover 상태 | `_hover` 시 사용 |
| `$primaryExBold` | Primary active 상태 | `_active` 시 사용 |
| `$primaryLight` | Primary 연한 버전 | 보조 강조 |
| `$primaryBg` | Primary 배경색 | 서브 버튼 배경 |
| `$primaryBgLight` | Primary 배경 연한 버전 | 호버 배경 |
| `$primaryBgBold` | Primary 배경 진한 버전 | 액티브 배경 |

### Secondary Colors (프로젝트별 상이)

| Token | Description | Usage |
|-------|-------------|-------|
| `$secondary` | 보조 브랜드 색상 | 보조 요소 |
| `$secondaryLight` | Secondary 연한 버전 | - |
| `$secondaryBold` | Secondary 진한 버전 | - |

### Text Colors

| Token | Description | Typical Value |
|-------|-------------|---------------|
| `$title` | 제목 텍스트 | 거의 검정 (#212020) |
| `$text` | 본문 텍스트 | 진한 회색 (#373333) |
| `$textLight` | 연한 텍스트 | 중간 회색 (#646468) |
| `$caption` | 캡션/보조 텍스트 | 회색 (#7D7F83) |
| `$captionLight` | 더 연한 캡션 | 연한 회색 (#8F8A88) |

### Background Colors

| Token | Description | Usage |
|-------|-------------|-------|
| `$background` | 페이지 배경 | body 배경 |
| `$containerBackground` | 컨테이너 배경 | 카드, 모달 배경 |
| `$base` | 베이스 색상 | 흰색 (#FFF) |
| `$negativeBase` | 반대 베이스 | 검정 (#000) |
| `$innerBg` | 내부 배경 | 중첩 컨테이너 |

### Gray Scale

| Token | Description | Typical Value |
|-------|-------------|---------------|
| `$gray100` | 가장 연한 회색 | #F1F1F1 |
| `$gray200` | 연한 회색 | #E5E5E6 |
| `$gray300` | 중간 회색 | #CBCAD1 |
| `$gray400` | 진한 회색 | #BBBAC5 |
| `$gray500` | 가장 진한 회색 | #9E9CA9 |

### Border Colors

| Token | Description | Usage |
|-------|-------------|-------|
| `$border` | 기본 테두리 | 일반 구분선 |
| `$borderLight` | 연한 테두리 | 미묘한 구분선 |
| `$borderBold` | 진한 테두리 | 강조 구분선 |

### Status Colors

| Token | Description | Typical Value |
|-------|-------------|---------------|
| `$success` | 성공/완료 | #4CAF50 |
| `$warning` | 경고 | #FAC450 |
| `$error` | 오류/삭제 | #D52943 |
| `$info` | 정보 | #2196F3 |
| `$link` | 링크 | #4D91FF |

### Tag Colors

| Token | Description | Usage |
|-------|-------------|-------|
| `$greenTag` | 초록 태그 텍스트 | 활성/완료 상태 |
| `$greenTagBg` | 초록 태그 배경 | - |
| `$blueTag` | 파란 태그 텍스트 | 정보 상태 |
| `$blueTagBg` | 파란 태그 배경 | - |
| `$orangeTag` | 주황 태그 텍스트 | 대기 상태 |
| `$orangeTagBg` | 주황 태그 배경 | - |
| `$redTag` | 빨간 태그 텍스트 | 경고/오류 상태 |
| `$redTagBg` | 빨간 태그 배경 | - |

## 프로젝트별 Primary 색상 예시

| Project | $primary | $secondary |
|---------|----------|------------|
| puzzle-fit | #772EDE (보라) | - |
| girok-space | #752D2D (적갈색) | #578F86 (민트) |

## 사용 예시

```tsx
// 버튼 스타일
<Button
  bg="$primary"
  _hover={{ bg: '$primaryBold' }}
  _active={{ bg: '$primaryExBold' }}
  _disabled={{ bg: '$gray200', color: '$gray400' }}
/>

// 태그 스타일
<Box bg="$greenTagBg" px="8px" py="4px" borderRadius="4px">
  <Text color="$greenTag" typography="tag">완료</Text>
</Box>

// 카드 스타일
<Box
  bg="$containerBackground"
  border="1px solid $border"
  borderRadius="12px"
  p="16px"
>
  <Text color="$title" typography="h6">제목</Text>
  <Text color="$text" typography="body">본문</Text>
  <Text color="$caption" typography="caption">캡션</Text>
</Box>
```

## devup.json 구조

각 프로젝트의 `devup.json` 파일 구조:

```json
{
  "theme": {
    "colors": {
      "light": {
        "primary": "#...",
        "secondary": "#...",
        // ... 기타 색상
      },
      "dark": {
        "primary": "#...",
        // ... 다크모드 색상
      }
    },
    "typography": {
      "h1": { "fontFamily": "...", "fontSize": "...", ... },
      "body": { ... },
      // ... 기타 타이포그래피
    }
  }
}
```
